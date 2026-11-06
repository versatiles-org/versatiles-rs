//! Accepting connections and serving them, with the limits that keep slow or
//! idle clients from holding the server.
//!
//! This replaces `axum::serve`, which builds hyper's connection handler without
//! a timer. Without one, hyper silently drops its default header-read timeout,
//! so a client could open connections and send their headers a byte at a time,
//! or nothing at all, for as long as it liked. Enough of those use up the
//! process's file descriptors, and the server stops accepting anyone.
//!
//! Three limits close that:
//! - **Connection cap:** past [`ConnectionLimits::max_connections`] open
//!   connections, no more are accepted until one closes. New clients wait in
//!   the kernel's backlog instead of failing with exhausted descriptors.
//! - **Header-read timeout:** an HTTP/1 request's headers must arrive within
//!   [`ConnectionLimits::header_read_timeout`]. hyper restarts this clock for
//!   every request, so it also closes keep-alive connections left idle.
//! - **Idle timeout:** a connection on which nothing is read or written for
//!   [`ConnectionLimits::idle_timeout`] is closed. This covers what the
//!   header-read timeout cannot: the bytes before hyper knows whether a
//!   connection speaks HTTP/1 or HTTP/2, idle HTTP/2 connections, and clients
//!   that stop reading their response.

use std::{
	future::Future,
	io,
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
	time::Duration,
};

use axum::{Router, body::Body, extract::Request};
use hyper::body::Incoming;
use hyper_util::{
	rt::{TokioExecutor, TokioIo, TokioTimer},
	server::{conn::auto::Builder, graceful::GracefulShutdown},
	service::TowerToHyperService,
};
use tokio::{
	io::{AsyncRead, AsyncWrite, ReadBuf},
	net::TcpListener,
	sync::{Semaphore, oneshot},
	time::{Instant, Sleep},
};
use tower::ServiceExt;

/// How many connections may be open at once.
///
/// Well above what a tile server behind a proxy sees, and below the common
/// hard limit on open files. Past it, connections wait to be accepted.
pub(super) const MAX_CONNECTIONS: usize = 2048;

/// How long an HTTP/1 client has to send a request's headers, and how long a
/// keep-alive connection may wait for its next request.
pub(super) const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a connection may go without a byte read or written.
///
/// Longer than the request timeout, so that a request still being answered
/// never counts as idle.
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// The limits [`serve`] applies to every connection.
#[derive(Clone, Copy, Debug)]
pub(super) struct ConnectionLimits {
	/// How many connections may be open at once.
	pub max_connections: usize,
	/// How long an HTTP/1 request's headers may take to arrive.
	pub header_read_timeout: Duration,
	/// How long a connection may go without a byte read or written.
	pub idle_timeout: Duration,
}

impl Default for ConnectionLimits {
	fn default() -> Self {
		ConnectionLimits {
			max_connections: MAX_CONNECTIONS,
			header_read_timeout: HEADER_READ_TIMEOUT,
			idle_timeout: IDLE_TIMEOUT,
		}
	}
}

/// Accept connections on `listener` and serve `router` on each, until
/// `shutdown` fires or its sender is dropped.
///
/// Then no more connections are accepted, and the ones that are open are
/// asked to finish: idle ones close, and requests in flight are answered.
/// This returns once all of them have closed.
pub(super) async fn serve(
	listener: TcpListener,
	router: Router,
	limits: ConnectionLimits,
	shutdown: oneshot::Receiver<()>,
) {
	let permits = Arc::new(Semaphore::new(limits.max_connections));
	let graceful = GracefulShutdown::new();

	let mut builder = Builder::new(TokioExecutor::new());
	builder
		.http1()
		.timer(TokioTimer::new())
		.header_read_timeout(limits.header_read_timeout);
	builder.http2().timer(TokioTimer::new());

	let service = router.map_request(|req: Request<Incoming>| req.map(Body::new));

	let mut shutdown = std::pin::pin!(shutdown);
	loop {
		let permit = tokio::select! {
			_ = &mut shutdown => break,
			permit = Arc::clone(&permits).acquire_owned() => permit.expect("the semaphore is never closed"),
		};

		let stream = tokio::select! {
			_ = &mut shutdown => break,
			accepted = listener.accept() => match accepted {
				Ok((stream, _)) => stream,
				Err(err) => {
					handle_accept_error(&err).await;
					continue;
				}
			},
		};

		let io = TokioIo::new(IdleTimeout::new(stream, limits.idle_timeout));
		let connection = builder
			.serve_connection(io, TowerToHyperService::new(service.clone()))
			.into_owned();
		let connection = graceful.watch(connection);

		tokio::spawn(async move {
			if let Err(err) = connection.await {
				log::trace!("connection closed with error: {err}");
			}
			drop(permit);
		});
	}

	drop(listener);
	graceful.shutdown().await;
}

/// Wait out a failed `accept`, as `axum::serve` does.
///
/// A connection that was reset before it was accepted is the client's
/// problem, so the next one is accepted at once. Anything else, most often
/// running out of file descriptors, gets a second to clear.
async fn handle_accept_error(err: &io::Error) {
	if matches!(
		err.kind(),
		io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset
	) {
		return;
	}
	log::error!("failed to accept a connection: {err}");
	tokio::time::sleep(Duration::from_secs(1)).await;
}

/// A stream that fails with [`io::ErrorKind::TimedOut`] once nothing has been
/// read from or written to it for a while.
///
/// The read or write is always tried first, and only a pending one consults
/// the clock. So a deadline that passed while hyper was not polling the
/// stream, for instance while a handler was running, never fails a read or
/// write that can make progress.
struct IdleTimeout<T> {
	inner: T,
	timeout: Duration,
	deadline: Pin<Box<Sleep>>,
}

impl<T> IdleTimeout<T> {
	fn new(inner: T, timeout: Duration) -> Self {
		IdleTimeout {
			inner,
			timeout,
			deadline: Box::pin(tokio::time::sleep(timeout)),
		}
	}

	/// Push the deadline out, after a read or write made progress.
	fn touch(&mut self) {
		let deadline = Instant::now() + self.timeout;
		self.deadline.as_mut().reset(deadline);
	}

	/// Turn a pending read or write into an error if the deadline has passed.
	fn pending_or_expired<R>(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<R>> {
		match self.deadline.as_mut().poll(cx) {
			Poll::Ready(()) => Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "connection idle"))),
			Poll::Pending => Poll::Pending,
		}
	}
}

impl<T: AsyncRead + Unpin> AsyncRead for IdleTimeout<T> {
	fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
		let this = self.get_mut();
		let filled = buf.filled().len();
		match Pin::new(&mut this.inner).poll_read(cx, buf) {
			Poll::Ready(result) => {
				if buf.filled().len() > filled {
					this.touch();
				}
				Poll::Ready(result)
			}
			Poll::Pending => this.pending_or_expired(cx),
		}
	}
}

impl<T: AsyncWrite + Unpin> AsyncWrite for IdleTimeout<T> {
	fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
		let this = self.get_mut();
		match Pin::new(&mut this.inner).poll_write(cx, buf) {
			Poll::Ready(result) => {
				if matches!(result, Ok(n) if n > 0) {
					this.touch();
				}
				Poll::Ready(result)
			}
			Poll::Pending => this.pending_or_expired(cx),
		}
	}

	fn poll_write_vectored(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		bufs: &[io::IoSlice<'_>],
	) -> Poll<io::Result<usize>> {
		let this = self.get_mut();
		match Pin::new(&mut this.inner).poll_write_vectored(cx, bufs) {
			Poll::Ready(result) => {
				if matches!(result, Ok(n) if n > 0) {
					this.touch();
				}
				Poll::Ready(result)
			}
			Poll::Pending => this.pending_or_expired(cx),
		}
	}

	fn is_write_vectored(&self) -> bool {
		self.inner.is_write_vectored()
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		let this = self.get_mut();
		match Pin::new(&mut this.inner).poll_flush(cx) {
			Poll::Ready(result) => Poll::Ready(result),
			Poll::Pending => this.pending_or_expired(cx),
		}
	}

	fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
	}
}

#[cfg(test)]
mod tests {
	use tokio::io::{AsyncReadExt, AsyncWriteExt};

	use super::*;

	/// Start [`serve`] on an ephemeral port with `limits`, answering `ok`.
	async fn start(limits: ConnectionLimits) -> (std::net::SocketAddr, oneshot::Sender<()>) {
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();
		let router = Router::new().route("/", axum::routing::get(|| async { "ok" }));
		let (tx, rx) = oneshot::channel();
		tokio::spawn(serve(listener, router, limits, rx));
		(addr, tx)
	}

	fn short_limits() -> ConnectionLimits {
		ConnectionLimits {
			max_connections: 4,
			header_read_timeout: Duration::from_millis(300),
			idle_timeout: Duration::from_millis(600),
		}
	}

	/// Read until the server closes the connection, and say how long that took.
	async fn time_until_closed(stream: &mut tokio::net::TcpStream) -> Duration {
		let started = std::time::Instant::now();
		let mut buf = Vec::new();
		let _ = tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut buf)).await;
		started.elapsed()
	}

	#[tokio::test]
	async fn answers_a_request() {
		let (addr, _tx) = start(short_limits()).await;
		let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
		stream
			.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
			.await
			.unwrap();
		let mut response = String::new();
		stream.read_to_string(&mut response).await.unwrap();
		assert!(response.starts_with("HTTP/1.1 200"), "{response}");
		assert!(response.ends_with("ok"), "{response}");
	}

	/// The slowloris case: headers started but never finished.
	#[tokio::test]
	async fn closes_a_connection_whose_headers_never_finish() {
		let (addr, _tx) = start(short_limits()).await;
		let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
		stream.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").await.unwrap();
		let waited = time_until_closed(&mut stream).await;
		assert!(waited < Duration::from_secs(5), "still open after {waited:?}");
	}

	/// Before the first byte, hyper does not know the protocol yet, and its
	/// header-read timeout has not started. The idle timeout covers that.
	#[tokio::test]
	async fn closes_a_connection_that_never_sends_anything() {
		let (addr, _tx) = start(short_limits()).await;
		let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
		let waited = time_until_closed(&mut stream).await;
		assert!(waited < Duration::from_secs(5), "still open after {waited:?}");
	}

	/// A prefix of the HTTP/2 preface keeps hyper guessing the protocol.
	#[tokio::test]
	async fn closes_a_connection_that_stalls_in_the_http2_preface() {
		let (addr, _tx) = start(short_limits()).await;
		let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
		stream.write_all(b"PRI * HTTP/2.0\r\n").await.unwrap();
		let waited = time_until_closed(&mut stream).await;
		assert!(waited < Duration::from_secs(5), "still open after {waited:?}");
	}

	/// Past the cap, a connection is not served until another one closes.
	#[tokio::test]
	async fn serves_no_more_connections_than_the_cap() {
		let limits = ConnectionLimits {
			max_connections: 2,
			header_read_timeout: Duration::from_secs(30),
			idle_timeout: Duration::from_secs(30),
		};
		let (addr, _tx) = start(limits).await;

		let mut held = Vec::new();
		for _ in 0..2 {
			let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
			stream.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
			let mut buf = [0u8; 12];
			stream.read_exact(&mut buf).await.unwrap();
			assert_eq!(&buf, b"HTTP/1.1 200");
			held.push(stream);
		}

		// The kernel completes the handshake, but the server does not accept it.
		let mut third = tokio::net::TcpStream::connect(addr).await.unwrap();
		third
			.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
			.await
			.unwrap();
		let mut buf = [0u8; 12];
		let early = tokio::time::timeout(Duration::from_millis(300), third.read_exact(&mut buf)).await;
		assert!(early.is_err(), "the third connection was served while two were open");

		drop(held.pop());
		tokio::time::timeout(Duration::from_secs(5), third.read_exact(&mut buf))
			.await
			.expect("the third connection was not served after one closed")
			.unwrap();
		assert_eq!(&buf, b"HTTP/1.1 200");
	}

	#[tokio::test]
	async fn shutting_down_closes_idle_connections() {
		let (addr, tx) = start(ConnectionLimits::default()).await;
		let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
		stream.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
		let mut buf = [0u8; 12];
		stream.read_exact(&mut buf).await.unwrap();

		tx.send(()).unwrap();
		let waited = time_until_closed(&mut stream).await;
		assert!(waited < Duration::from_secs(5), "still open after {waited:?}");
	}
}
