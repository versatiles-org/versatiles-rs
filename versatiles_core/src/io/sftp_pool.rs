//! Connection pool for SFTP sources.
//!
//! Opening one full SSH connection per source does not scale: a burst of
//! parallel opens to the same server trips `sshd`'s `MaxStartups` limit and
//! the server rejects connections.
//!
//! The fix is to rate-limit connection *establishment* — handshakes run a few
//! at a time, staying under `MaxStartups` — without throttling steady-state
//! concurrency. Each server may hold many connections (up to a cap), so
//! concurrent reads spread across them instead of being funnelled through one
//! lock; a source reuses an existing connection only once the cap is reached.

use std::{
	collections::HashMap,
	path::{Path, PathBuf},
	sync::{
		Arc, OnceLock,
		atomic::{AtomicU64, Ordering},
	},
	time::Instant,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use futures::{StreamExt, stream::FuturesOrdered};
use reqwest::Url;
use russh_sftp::{client::error::Error as SftpError, protocol::StatusCode};
use tokio::sync::{Mutex, Notify, Semaphore};

use super::sftp_utils::{self, RawSftp, SshHandle};
use crate::{Blob, ByteRange};

/// How many read requests one range keeps in flight at most. Only a range
/// longer than one request has more than one, and none reaches past its end.
const MAX_READS_IN_FLIGHT: usize = 16;

/// A file opened on a connection.
struct OpenFile {
	/// The server's handle for it.
	handle: String,
	/// Its size when it was opened: a reply that ends short of it was cut by
	/// the server, not by the end of the file.
	size: u64,
}

/// Monotonic id for log correlation across pool / connection / read messages.
fn next_connection_id() -> u64 {
	static NEXT: AtomicU64 = AtomicU64::new(0);
	NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Default maximum SSH connections kept open per server.
///
/// Operations on a single connection are serialized behind its lock, so this
/// is also the per-server ceiling on *concurrent* reads.
///
/// Kept conservative because some SFTP servers cap simultaneous connections
/// aggressively — e.g. a Hetzner Storage Box allows only 10. Override with the
/// `VERSATILES_SFTP_MAX_CONNECTIONS` environment variable for servers that
/// allow more (more read concurrency) or fewer.
const DEFAULT_CONNECTIONS_PER_SERVER: usize = 8;

/// Per-server connection cap: `VERSATILES_SFTP_MAX_CONNECTIONS` if set to a
/// positive integer, otherwise [`DEFAULT_CONNECTIONS_PER_SERVER`]. Read once.
fn connections_per_server() -> usize {
	static CAP: OnceLock<usize> = OnceLock::new();
	*CAP.get_or_init(|| {
		std::env::var("VERSATILES_SFTP_MAX_CONNECTIONS")
			.ok()
			.and_then(|value| value.parse::<usize>().ok())
			.filter(|&n| n > 0)
			.unwrap_or(DEFAULT_CONNECTIONS_PER_SERVER)
	})
}

/// Maximum SSH handshakes performed concurrently. Kept below `sshd`'s default
/// `MaxStartups` threshold (10) so a burst of opens is never rejected, while
/// still letting handshakes proceed in parallel rather than one at a time.
const MAX_CONCURRENT_OPENS: usize = 8;

/// Process-wide handshake throttle.
///
/// A `tokio::sync::Semaphore`; the ssh2 implementation had to hand-roll a
/// counting semaphore out of `Mutex` and `Condvar` because it could not await.
fn open_throttle() -> &'static Semaphore {
	static THROTTLE: OnceLock<Semaphore> = OnceLock::new();
	THROTTLE.get_or_init(|| Semaphore::new(MAX_CONCURRENT_OPENS))
}

/// Identifies one SSH endpoint. Sources sharing a key share connections.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ServerKey {
	host: String,
	port: u16,
	username: String,
}

impl ServerKey {
	fn from_url(url: &Url) -> Result<Self> {
		Ok(ServerKey {
			host: url.host_str().context("SFTP URL has no host")?.to_string(),
			port: url.port().unwrap_or(22),
			// Mirror the default applied by `sftp_utils::open_session`.
			username: if url.username().is_empty() {
				"root"
			} else {
				url.username()
			}
			.to_string(),
		})
	}
}

/// The session-bound state of a connection: the SSH session, its SFTP channel,
/// and the file handles opened on it. Replaced wholesale on every reconnect.
struct LiveConnection {
	// Kept alive for the lifetime of the SFTP channel: dropping the handle
	// closes the transport underneath it.
	_session: SshHandle,
	/// Shared with the tasks that close files, which run without the lock.
	sftp: Arc<russh_sftp::client::RawSftpSession>,
	/// The longest read the server answers in full. Starts at what the server
	/// says, and shrinks to the length of a reply that it cut short.
	read_len: u32,
	/// Open files, by id. Rebuilt from [`ConnectionInner::registered`] on
	/// every reconnect.
	files: HashMap<u64, OpenFile>,
}

impl LiveConnection {
	fn new(session: SshHandle, sftp: RawSftp, files: HashMap<u64, OpenFile>) -> Self {
		LiveConnection {
			_session: session,
			sftp: Arc::new(sftp.session),
			read_len: sftp.max_read_len,
			files,
		}
	}
}

/// Opens `path` for reading, with its size.
async fn open_file(sftp: &russh_sftp::client::RawSftpSession, path: &str) -> Result<OpenFile> {
	use russh_sftp::protocol::{FileAttributes, OpenFlags};

	let size = sftp
		.stat(path.to_owned())
		.await
		.with_context(|| format!("failed to stat remote file {path:?}"))?
		.attrs
		.size
		.unwrap_or(0);
	let handle = sftp
		.open(path.to_owned(), OpenFlags::READ, FileAttributes::default())
		.await
		.with_context(|| format!("failed to open remote file {path:?}"))?
		.handle;
	Ok(OpenFile { handle, size })
}

/// Reads `range` of `file`, asking for no byte outside it.
///
/// One request per `read_len` bytes, up to [`MAX_READS_IN_FLIGHT`] at a time.
/// A server may answer a request with less than was asked for; the rest is
/// then asked for again, and the requests behind it, which no longer start
/// where the data ends, are dropped. Where such a reply ends inside the file,
/// the server cut it, and `read_len` shrinks to its length, so that the next
/// requests are ones the server answers in full.
async fn read_exact(
	sftp: &russh_sftp::client::RawSftpSession,
	file: &OpenFile,
	range: &ByteRange,
	read_len: &mut u32,
) -> Result<Vec<u8>> {
	let end = range
		.offset
		.checked_add(range.length)
		.with_context(|| format!("range {range} ends beyond 2^64"))?;
	let mut buffer = Vec::with_capacity(usize::try_from(range.length)?);
	let mut position = range.offset;
	let mut next = range.offset;
	let mut in_flight = FuturesOrdered::new();

	while position < end {
		while in_flight.len() < MAX_READS_IN_FLIGHT && next < end {
			let (start, len) = (next, u32::try_from(end.saturating_sub(next).min(u64::from(*read_len)))?);
			let handle = file.handle.as_str();
			in_flight.push_back(async move { (start, len, sftp.read(handle, start, len).await) });
			// At most `end`, since `len` is at most what is left of the range.
			next = next.saturating_add(u64::from(len));
		}
		let (start, len, reply) = in_flight
			.next()
			.await
			.context("no read request in flight before the end of the range")?;
		ensure!(
			start == position,
			"the reply for {start} came while waiting for {position}"
		);
		let data = match reply {
			Ok(data) => data.data,
			Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => {
				bail!("the file ends at byte {position}, before the end of {range}")
			}
			Err(error) => return Err(error.into()),
		};
		let received = u32::try_from(data.len()).unwrap_or(u32::MAX);
		ensure!(
			received > 0 && received <= len,
			"the server answered a read of {len} bytes at {start} with {received}"
		);
		buffer.extend_from_slice(&data);
		// At most `end`, since `received` is at most what was asked for.
		position = position.saturating_add(u64::from(received));

		if received < len {
			if position < file.size {
				*read_len = (*read_len).min(received);
			}
			in_flight = FuturesOrdered::new();
			next = position;
		}
	}
	Ok(buffer)
}

/// The mutable state behind a connection's lock.
///
/// One SFTP channel serves every file on the connection. Reads, opens and
/// reconnects all happen while this is locked, so that a reconnect never
/// swaps the session out from under a read.
struct ConnectionInner {
	/// The live SSH session, or `None` while a reconnect is in progress or
	/// after one failed (the next read then triggers a fresh reconnect).
	live: Option<LiveConnection>,
	/// Remote path of every registered file, by id. Kept outside `live` so it
	/// survives reconnects — even failed ones — and stays the durable source
	/// of truth for re-opening the file handles.
	registered: HashMap<u64, String>,
	/// Bumped on every successful reconnect. Captured before a read so that
	/// many sources racing to reconnect the same dropped connection trigger
	/// just one actual reconnect.
	generation: u64,
	/// Monotonic id source for registered files; never reused, even after removal.
	next_file_id: u64,
	/// Wall-clock instant of the last successful read (or of the open /
	/// reconnect, if no read has happened yet). Logged on failures so we can
	/// tell whether the connection died while active or while sitting idle.
	last_used: Instant,
}

/// One pooled SSH connection, shared by every source it serves.
pub struct Connection {
	/// Process-wide unique id used in log messages to correlate pool, open,
	/// reconnect and read events across threads.
	id: u64,
	inner: Mutex<ConnectionInner>,
	/// Re-open data for reconnects (host/port/user/password; path is unused).
	url: Url,
	identity_file: Option<PathBuf>,
}

impl Connection {
	async fn open(url: &Url, identity_file: Option<&Path>) -> Result<Arc<Connection>> {
		let id = next_connection_id();
		let display = sftp_utils::display_name(url);
		log::debug!("[sftp conn {id}] opening SSH+SFTP session to '{display}'");
		let started = Instant::now();
		let session = sftp_utils::open_session(url, identity_file).await?;
		let sftp = sftp_utils::open_raw_sftp(&session).await?;
		log::debug!(
			"[sftp conn {id}] session ready in {:.2}s",
			started.elapsed().as_secs_f32()
		);
		Ok(Arc::new(Connection {
			id,
			inner: Mutex::new(ConnectionInner {
				live: Some(LiveConnection::new(session, sftp, HashMap::new())),
				registered: HashMap::new(),
				generation: 0,
				next_file_id: 0,
				last_used: Instant::now(),
			}),
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
		}))
	}

	/// Open `path` on this connection's shared SFTP channel.
	///
	/// Returns the file id (used for later reads) and the file size.
	///
	/// # Errors
	/// Returns an error if the connection is not established, or if the remote
	/// file cannot be stat'ed or opened.
	pub async fn register(&self, path: &str) -> Result<(u64, u64)> {
		let mut inner = self.inner.lock().await;
		let id = inner.next_file_id;
		let live = inner
			.live
			.as_mut()
			.ok_or_else(|| anyhow!("SFTP connection is not established"))?;
		let file = open_file(&live.sftp, path).await?;
		let size = file.size;
		live.files.insert(id, file);
		inner.registered.insert(id, path.to_owned());
		// Saturating would hand out the last id twice; 2^64 opens never happen,
		// but if they did, this is an error rather than two files sharing an id.
		inner.next_file_id = id.checked_add(1).context("SFTP file ids are exhausted")?;
		Ok((id, size))
	}

	/// Drop a file handle once its source is gone. Best-effort.
	pub async fn unregister(&self, id: u64) {
		let closing = {
			let mut inner = self.inner.lock().await;
			inner.registered.remove(&id);
			inner
				.live
				.as_mut()
				.and_then(|live| Some((Arc::clone(&live.sftp), live.files.remove(&id)?)))
		};
		// Outside the lock: on a dead connection the reply never comes, and
		// the reads of other files must not wait for it to time out.
		if let Some((sftp, file)) = closing
			&& let Err(error) = sftp.close(file.handle).await
		{
			log::debug!("[sftp conn {}] closing file {id} failed: {error}", self.id);
		}
	}

	/// The current reconnect generation. Capture it before a read so a
	/// failing read can request a reconnect without racing its siblings.
	pub async fn generation(&self) -> u64 {
		self.inner.lock().await.generation
	}

	/// Read `range` from the file registered under `id`.
	///
	/// # Errors
	/// Returns an error if the connection is not established, the id is not
	/// registered, or the read fails.
	pub async fn read_range(&self, id: u64, range: &ByteRange) -> Result<Blob> {
		let mut inner = self.inner.lock().await;
		let idle = inner.last_used.elapsed();

		let read_result: Result<Blob> = async {
			let live = inner
				.live
				.as_mut()
				.ok_or_else(|| anyhow!("SFTP connection is not established"))?;
			let file = live
				.files
				.get(&id)
				.ok_or_else(|| anyhow!("SFTP file id {id} is not registered"))?;
			let buffer = read_exact(&live.sftp, file, range, &mut live.read_len).await?;
			Ok(Blob::from(buffer))
		}
		.await;

		match read_result {
			Ok(blob) => {
				inner.last_used = Instant::now();
				Ok(blob)
			}
			Err(e) => {
				// Idle time on a failure tells us whether the connection died
				// while active or after sitting idle — the latter suggests the
				// server is closing idle sessions (e.g. SSH ClientAliveInterval).
				log::debug!(
					"[sftp conn {}] read {range} failed after {:.1}s idle: {e}",
					self.id,
					idle.as_secs_f32(),
				);
				Err(e)
			}
		}
	}

	/// Rebuild the SSH session and SFTP channel and re-open every registered
	/// file.
	///
	/// A no-op if another task already reconnected past `seen_generation`,
	/// so many sources sharing one dropped connection trigger a single
	/// reconnect rather than one per source.
	///
	/// # Errors
	/// Returns an error if the session cannot be re-established or a registered
	/// file cannot be reopened.
	pub async fn reconnect(&self, seen_generation: u64) -> Result<()> {
		let mut inner = self.inner.lock().await;
		if inner.generation != seen_generation {
			// A sibling already reconnected this connection — nothing to do.
			return Ok(());
		}
		log::info!(
			"[sftp conn {}] reconnecting (idle for {:.1}s) to '{}'",
			self.id,
			inner.last_used.elapsed().as_secs_f32(),
			sftp_utils::display_name(&self.url)
		);

		// Drop the old session *before* opening the new one, so the two never
		// both count against the server's simultaneous-connection limit.
		inner.live = None;

		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref()).await?;
		let sftp = sftp_utils::open_raw_sftp(&session).await?;
		let mut files = HashMap::with_capacity(inner.registered.len());
		for (id, path) in &inner.registered {
			let file = open_file(&sftp.session, path)
				.await
				.with_context(|| format!("failed to reopen remote file {path:?}"))?;
			files.insert(*id, file);
		}
		inner.live = Some(LiveConnection::new(session, sftp, files));
		// Only ever compared for equality, so wrapping keeps it working.
		inner.generation = inner.generation.wrapping_add(1);
		inner.last_used = Instant::now();
		log::debug!(
			"[sftp conn {}] reconnect complete (generation {})",
			self.id,
			inner.generation
		);
		Ok(())
	}
}

/// The connections held for one server.
#[derive(Default)]
struct ServerPool {
	/// Established connections, handed out round-robin once the cap is reached.
	connections: Vec<Arc<Connection>>,
	/// Connections currently mid-handshake — counted toward the cap so racing
	/// `acquire` calls cannot collectively overshoot it.
	opening: usize,
	/// Round-robin cursor into `connections`.
	next: usize,
}

static POOL: OnceLock<Mutex<HashMap<ServerKey, ServerPool>>> = OnceLock::new();
static POOL_READY: OnceLock<Notify> = OnceLock::new();

fn pool() -> &'static Mutex<HashMap<ServerKey, ServerPool>> {
	POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Signalled whenever a connection finishes opening, waking `acquire` calls
/// waiting at the cap for a connection to become reusable.
fn pool_ready() -> &'static Notify {
	POOL_READY.get_or_init(Notify::new)
}

/// What an `acquire` iteration decided to do, computed under the pool lock.
enum Decision {
	/// Open a fresh connection (a slot was reserved via `opening`).
	Open,
	/// Reuse an already-established connection.
	Reuse(Arc<Connection>),
	/// At the cap with every connection still mid-handshake — wait.
	Wait,
}

/// Get a pooled connection for `url`.
///
/// A new connection is opened while the server is below its connection cap
/// (see [`connections_per_server`]); beyond that an existing connection is
/// reused round-robin. The handshake runs *outside* the pool lock, rate-limited by
/// [`open_throttle`] — so neither opening nor reading is serialized through a
/// single global lock, while the handshake burst still stays under `sshd`'s
/// `MaxStartups` limit.
///
/// # Errors
/// Returns an error if the URL has no host or the connection cannot be opened.
pub async fn acquire(url: &Url, identity_file: Option<&Path>) -> Result<Arc<Connection>> {
	let key = ServerKey::from_url(url)?;

	// `Condvar::wait` used to release the pool lock and register for the wakeup
	// as one atomic step. `Notify` splits those, and a `Notified` future does not
	// register until it is polled — so creating it before the lock is not enough.
	// Without the `enable()` below, a connection finishing between the decision
	// and the `.await` would notify nobody, and this call would wait for a wakeup
	// that had already happened.
	let ready = pool_ready().notified();
	tokio::pin!(ready);

	loop {
		ready.as_mut().enable();

		let decision = {
			let mut guard = pool().lock().await;
			let server = guard.entry(key.clone()).or_default();
			if server.connections.len().saturating_add(server.opening) < connections_per_server() {
				server.opening = server.opening.saturating_add(1);
				Decision::Open
			} else if server.connections.is_empty() {
				Decision::Wait
			} else {
				let connection = Arc::clone(&server.connections[server.next]);
				// Connections are only ever added, so `next` stays in bounds.
				let next = server.next.saturating_add(1);
				server.next = if next < server.connections.len() { next } else { 0 };
				Decision::Reuse(connection)
			}
		};

		match decision {
			Decision::Reuse(connection) => {
				log::debug!("[sftp pool] {}: reusing conn {}", key.host, connection.id);
				return Ok(connection);
			}
			Decision::Wait => {
				log::debug!(
					"[sftp pool] {}: at cap ({}), waiting for a connection",
					key.host,
					connections_per_server()
				);
				ready.as_mut().await;
				// A consumed `Notified` never fires again; the next iteration
				// needs a fresh one to register with.
				ready.set(pool_ready().notified());
			}
			Decision::Open => {
				// Handshake outside the pool lock, throttled to stay under MaxStartups.
				let result = {
					let _permit = open_throttle()
						.acquire()
						.await
						.map_err(|e| anyhow!("the SFTP handshake throttle was closed: {e}"))?;
					Connection::open(url, identity_file).await
				};

				let outcome = {
					let mut guard = pool().lock().await;
					let server = guard.entry(key.clone()).or_default();
					server.opening = server.opening.saturating_sub(1);
					match result {
						Ok(connection) => {
							server.connections.push(Arc::clone(&connection));
							log::debug!(
								"[sftp pool] {}: opened conn {}, pool now {} connection(s) (cap {})",
								key.host,
								connection.id,
								server.connections.len(),
								connections_per_server()
							);
							Ok(connection)
						}
						Err(e) => {
							log::debug!("[sftp pool] {}: open failed: {e}", key.host);
							Err(e)
						}
					}
				};
				// Wake anyone waiting at the cap: a connection appeared, or a
				// reserved slot was freed by a failed open.
				pool_ready().notify_waiters();
				return outcome;
			}
		}
	}
}

/// Number of pooled connections currently held for `url`'s server.
#[cfg(test)]
async fn connection_count(url: &Url) -> Result<usize> {
	let key = ServerKey::from_url(url)?;
	let guard = pool().lock().await;
	Ok(guard.get(&key).map_or(0, |server| server.connections.len()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_server_key_from_url_defaults() {
		let url = Url::parse("sftp://host/path").unwrap();
		let key = ServerKey::from_url(&url).unwrap();
		assert_eq!(key.host, "host");
		assert_eq!(key.port, 22);
		assert_eq!(key.username, "root");
	}

	#[test]
	fn test_server_key_from_url_explicit() {
		let url = Url::parse("sftp://user@host:2222/path").unwrap();
		let key = ServerKey::from_url(&url).unwrap();
		assert_eq!(key.host, "host");
		assert_eq!(key.port, 2222);
		assert_eq!(key.username, "user");
	}

	#[test]
	fn test_server_key_ignores_path() {
		let a = ServerKey::from_url(&Url::parse("sftp://host/a").unwrap()).unwrap();
		let b = ServerKey::from_url(&Url::parse("sftp://host/b").unwrap()).unwrap();
		assert_eq!(a, b);
	}

	#[test]
	fn test_server_key_missing_host() {
		let url = Url::parse("sftp:///path").unwrap();
		assert!(ServerKey::from_url(&url).is_err());
	}

	#[cfg(feature = "sftp")]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn caps_connections_per_server() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"hello").await;
			let url = server.url("/a.bin");

			// Acquire more sources than the per-server cap, concurrently — the
			// surplus must reuse existing connections rather than open new
			// ones, even when many `acquire` calls race.
			let cap = connections_per_server();
			let total = cap + 4;
			let mut handles = Vec::with_capacity(total);
			for _ in 0..total {
				let url = url.clone();
				handles.push(tokio::spawn(async move { acquire(&url, None).await }));
			}
			let mut acquired = Vec::with_capacity(total);
			for handle in handles {
				acquired.push(handle.await.unwrap().unwrap());
			}

			assert_eq!(acquired.len(), total);
			assert_eq!(connection_count(&url).await.unwrap(), cap);
		}

		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn many_files_share_one_connection() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"aaaa").await;
			server.write_file("/b.bin", b"bbbbbb").await;
			let url = server.url("/a.bin");

			let connection = acquire(&url, None).await.unwrap();
			let (id_a, size_a) = connection.register("/a.bin").await.unwrap();
			let (id_b, size_b) = connection.register("/b.bin").await.unwrap();
			assert_eq!(size_a, 4);
			assert_eq!(size_b, 6);
			assert_eq!(
				connection
					.read_range(id_a, &ByteRange::new(0, 4))
					.await
					.unwrap()
					.as_slice(),
				b"aaaa"
			);
			assert_eq!(
				connection
					.read_range(id_b, &ByteRange::new(2, 4))
					.await
					.unwrap()
					.as_slice(),
				b"bbbb"
			);
			connection.unregister(id_a).await;
			assert!(connection.read_range(id_a, &ByteRange::new(0, 4)).await.is_err());
		}

		/// Bytes that differ from one offset to the next, so that a misplaced
		/// chunk shows.
		fn pattern(len: usize) -> Vec<u8> {
			(0..len)
				.map(|i| u8::try_from((i * 7 + i / 251) % 256).unwrap())
				.collect()
		}

		async fn read(connection: &Connection, id: u64, offset: u64, length: u64) -> Result<Vec<u8>> {
			Ok(connection
				.read_range(id, &ByteRange::new(offset, length))
				.await?
				.as_slice()
				.to_vec())
		}

		/// #331: russh-sftp's `File` asked for up to 16 chunks of 256 KiB on
		/// every read, and a read of a few KB moved megabytes.
		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn a_small_read_transfers_only_its_bytes() {
			let server = TestSftpServer::start().await;
			let data = pattern(4 << 20);
			server.write_file("/big.bin", &data).await;
			let connection = acquire(&server.url("/big.bin"), None).await.unwrap();
			let (id, _) = connection.register("/big.bin").await.unwrap();

			assert_eq!(
				read(&connection, id, 1_000_000, 3000).await.unwrap(),
				data[1_000_000..1_003_000]
			);
			assert_eq!(
				read(&connection, id, 2_000_000, 3000).await.unwrap(),
				data[2_000_000..2_003_000]
			);
			assert_eq!(server.bytes_read(), 6000);
		}

		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn a_range_longer_than_one_request_is_read_to_its_end_and_no_further() {
			let server = TestSftpServer::start().await;
			let data = pattern(2 << 20);
			server.write_file("/big.bin", &data).await;
			let connection = acquire(&server.url("/big.bin"), None).await.unwrap();
			let (id, _) = connection.register("/big.bin").await.unwrap();

			let range = 12_345..1_012_345;
			assert_eq!(
				read(&connection, id, range.start as u64, range.len() as u64)
					.await
					.unwrap(),
				data[range.clone()]
			);
			assert_eq!(server.bytes_read(), range.len() as u64);
		}

		/// A server that answers every read with less than was asked for,
		/// without saying so beforehand.
		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn reads_shrink_to_what_the_server_answers() {
			let server = TestSftpServer::start().await;
			let data = pattern(1 << 20);
			server.write_file("/big.bin", &data).await;
			server.set_max_read_len(1000);
			let connection = acquire(&server.url("/big.bin"), None).await.unwrap();
			let (id, _) = connection.register("/big.bin").await.unwrap();

			assert_eq!(read(&connection, id, 5, 100_000).await.unwrap(), data[5..100_005]);
			// The first requests, sent before the first reply came back, are
			// answered in part, and all but the first of those are dropped.
			let first = server.bytes_read();
			assert!(first < 100_000 + 16 * 1000, "{first} bytes for a read of 100000");

			// From then on, the requests are as long as the replies.
			assert_eq!(
				read(&connection, id, 500_000, 50_000).await.unwrap(),
				data[500_000..550_000]
			);
			assert_eq!(server.bytes_read() - first, 50_000);
		}

		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn a_range_beyond_the_end_of_the_file_is_an_error() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"hello").await;
			let connection = acquire(&server.url("/a.bin"), None).await.unwrap();
			let (id, _) = connection.register("/a.bin").await.unwrap();

			let error = read(&connection, id, 3, 10).await.unwrap_err();
			assert!(error.to_string().contains("ends at byte 5"), "{error}");
			assert_eq!(read(&connection, id, 5, 0).await.unwrap(), b"");
		}

		#[tokio::test(flavor = "multi_thread")]
		#[serial_test::serial]
		async fn reconnect_reopens_registered_files() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"hello").await;
			let url = server.url("/a.bin");

			let connection = acquire(&url, None).await.unwrap();
			let (id, _) = connection.register("/a.bin").await.unwrap();
			let generation = connection.generation().await;
			connection.reconnect(generation).await.unwrap();
			assert_eq!(connection.generation().await, generation + 1);
			// The file handle survives the reconnect.
			assert_eq!(
				connection
					.read_range(id, &ByteRange::new(0, 5))
					.await
					.unwrap()
					.as_slice(),
				b"hello"
			);
			// A stale generation makes reconnect a no-op.
			connection.reconnect(generation).await.unwrap();
			assert_eq!(connection.generation().await, generation + 1);
		}
	}
}
