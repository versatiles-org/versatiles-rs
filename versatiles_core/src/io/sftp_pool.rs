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

use super::sftp_utils;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, anyhow};
use reqwest::Url;
use ssh2::{Session, Sftp};
use std::{
	collections::HashMap,
	io::{Read, Seek, SeekFrom},
	path::{Path, PathBuf},
	sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock},
};

/// Maximum SSH connections kept open per server. Operations on a single
/// libssh2 session are serialized (the session is not thread-safe), so this is
/// also the per-server ceiling on *concurrent* reads. Kept well below the
/// simultaneous-connection limit of typical SFTP servers.
const CONNECTIONS_PER_SERVER: usize = 16;

/// Maximum SSH handshakes performed concurrently. Kept below `sshd`'s default
/// `MaxStartups` threshold (10) so a burst of opens is never rejected, while
/// still letting handshakes proceed in parallel rather than one at a time.
const MAX_CONCURRENT_OPENS: usize = 8;

/// A minimal counting semaphore (std only) bounding concurrent SSH handshakes.
struct OpenThrottle {
	permits: Mutex<usize>,
	released: Condvar,
}

impl OpenThrottle {
	/// Block until a permit is free; the returned guard releases it on drop.
	fn acquire(&self) -> OpenPermit<'_> {
		let mut permits = self.permits.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
		while *permits == 0 {
			permits = self
				.released
				.wait(permits)
				.unwrap_or_else(std::sync::PoisonError::into_inner);
		}
		*permits -= 1;
		OpenPermit { throttle: self }
	}
}

/// RAII permit from [`OpenThrottle::acquire`]; returns the permit on drop.
struct OpenPermit<'a> {
	throttle: &'a OpenThrottle,
}

impl Drop for OpenPermit<'_> {
	fn drop(&mut self) {
		let mut permits = self
			.throttle
			.permits
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner);
		*permits += 1;
		self.throttle.released.notify_one();
	}
}

/// Process-wide handshake throttle.
fn open_throttle() -> &'static OpenThrottle {
	static THROTTLE: OnceLock<OpenThrottle> = OnceLock::new();
	THROTTLE.get_or_init(|| OpenThrottle {
		permits: Mutex::new(MAX_CONCURRENT_OPENS),
		released: Condvar::new(),
	})
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

/// One open file handle on a pooled connection.
struct OpenFile {
	path: PathBuf,
	file: ssh2::File,
}

/// The mutable state behind a connection's lock.
///
/// libssh2 sessions are not safe for concurrent use, so every operation —
/// reading, opening a file, reconnecting — happens while this is locked.
struct ConnectionInner {
	// Kept alive for the lifetime of the SFTP channel.
	_session: Session,
	sftp: Sftp,
	/// Every file handle opened on this connection, kept centrally so a
	/// reconnect (which invalidates the whole session) can re-open them all.
	files: HashMap<u64, OpenFile>,
	/// Bumped on every reconnect. Captured before a read so that 50 sources
	/// racing to reconnect the same dropped connection trigger just one
	/// actual reconnect.
	generation: u64,
	/// Monotonic id source for `files`; never reused, even after removal.
	next_file_id: u64,
}

/// One pooled SSH connection, shared by every source it serves.
pub struct Connection {
	inner: Mutex<ConnectionInner>,
	/// Re-open data for reconnects (host/port/user/password; path is unused).
	url: Url,
	identity_file: Option<PathBuf>,
}

impl Connection {
	fn open(url: &Url, identity_file: Option<&Path>) -> Result<Arc<Connection>> {
		let session = sftp_utils::open_session(url, identity_file)?;
		let sftp = session.sftp()?;
		Ok(Arc::new(Connection {
			inner: Mutex::new(ConnectionInner {
				_session: session,
				sftp,
				files: HashMap::new(),
				generation: 0,
				next_file_id: 0,
			}),
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
		}))
	}

	fn lock(&self) -> Result<MutexGuard<'_, ConnectionInner>> {
		self
			.inner
			.lock()
			.map_err(|e| anyhow!("SFTP connection lock poisoned: {e}"))
	}

	/// Open `path` on this connection's shared SFTP channel.
	///
	/// Returns the file id (used for later reads) and the file size.
	pub fn register(&self, path: &Path) -> Result<(u64, u64)> {
		let mut inner = self.lock()?;
		let size = inner
			.sftp
			.stat(path)
			.with_context(|| format!("failed to stat remote file {path:?}"))?
			.size
			.unwrap_or(0);
		let file = inner
			.sftp
			.open(path)
			.with_context(|| format!("failed to open remote file {path:?}"))?;
		let id = inner.next_file_id;
		inner.next_file_id += 1;
		inner.files.insert(
			id,
			OpenFile {
				path: path.to_path_buf(),
				file,
			},
		);
		Ok((id, size))
	}

	/// Drop a file handle once its source is gone. Best-effort.
	pub fn unregister(&self, id: u64) {
		if let Ok(mut inner) = self.lock() {
			inner.files.remove(&id);
		}
	}

	/// The current reconnect generation. Capture it before a read so a
	/// failing read can request a reconnect without racing its siblings.
	pub fn generation(&self) -> Result<u64> {
		Ok(self.lock()?.generation)
	}

	/// Read `range` from the file registered under `id`.
	pub fn read_range(&self, id: u64, range: &ByteRange) -> Result<Blob> {
		let mut inner = self.lock()?;
		let open_file = inner
			.files
			.get_mut(&id)
			.ok_or_else(|| anyhow!("SFTP file id {id} is not registered"))?;
		open_file.file.seek(SeekFrom::Start(range.offset))?;
		let mut buffer = vec![0u8; usize::try_from(range.length)?];
		open_file.file.read_exact(&mut buffer)?;
		Ok(Blob::from(buffer))
	}

	/// Rebuild the SSH session and SFTP channel and re-open every registered
	/// file.
	///
	/// A no-op if another thread already reconnected past `seen_generation`,
	/// so many sources sharing one dropped connection trigger a single
	/// reconnect rather than one per source.
	pub fn reconnect(&self, seen_generation: u64) -> Result<()> {
		let mut inner = self.lock()?;
		if inner.generation != seen_generation {
			// A sibling already reconnected this connection — nothing to do.
			return Ok(());
		}
		log::info!(
			"reconnecting pooled SFTP session to '{}'",
			sftp_utils::display_name(&self.url)
		);

		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref())?;
		let sftp = session.sftp()?;
		let mut files = HashMap::with_capacity(inner.files.len());
		for (id, open_file) in &inner.files {
			let file = sftp
				.open(&open_file.path)
				.with_context(|| format!("failed to reopen remote file {:?}", open_file.path))?;
			files.insert(
				*id,
				OpenFile {
					path: open_file.path.clone(),
					file,
				},
			);
		}
		inner._session = session;
		inner.sftp = sftp;
		inner.files = files;
		inner.generation += 1;
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
static POOL_READY: OnceLock<Condvar> = OnceLock::new();

fn pool() -> &'static Mutex<HashMap<ServerKey, ServerPool>> {
	POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Signalled whenever a connection finishes opening, waking `acquire` calls
/// waiting at the cap for a connection to become reusable.
fn pool_ready() -> &'static Condvar {
	POOL_READY.get_or_init(Condvar::new)
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
/// A new connection is opened while the server is below
/// [`CONNECTIONS_PER_SERVER`]; beyond that an existing connection is reused
/// round-robin. The handshake runs *outside* the pool lock, rate-limited by
/// [`open_throttle`] — so neither opening nor reading is serialized through a
/// single global lock, while the handshake burst still stays under `sshd`'s
/// `MaxStartups` limit.
pub fn acquire(url: &Url, identity_file: Option<&Path>) -> Result<Arc<Connection>> {
	let key = ServerKey::from_url(url)?;
	let mut guard = pool().lock().map_err(|e| anyhow!("SFTP pool lock poisoned: {e}"))?;

	loop {
		let decision = {
			let server = guard.entry(key.clone()).or_default();
			if server.connections.len() + server.opening < CONNECTIONS_PER_SERVER {
				server.opening += 1;
				Decision::Open
			} else if server.connections.is_empty() {
				Decision::Wait
			} else {
				let connection = Arc::clone(&server.connections[server.next]);
				server.next = (server.next + 1) % server.connections.len();
				Decision::Reuse(connection)
			}
		};

		match decision {
			Decision::Reuse(connection) => return Ok(connection),
			Decision::Wait => {
				guard = pool_ready()
					.wait(guard)
					.map_err(|e| anyhow!("SFTP pool lock poisoned: {e}"))?;
			}
			Decision::Open => {
				drop(guard);

				// Handshake outside the pool lock, throttled to stay under MaxStartups.
				let result = {
					let _permit = open_throttle().acquire();
					Connection::open(url, identity_file)
				};

				let outcome = {
					let mut g = pool().lock().map_err(|e| anyhow!("SFTP pool lock poisoned: {e}"))?;
					let server = g.entry(key.clone()).or_default();
					server.opening -= 1;
					match result {
						Ok(connection) => {
							server.connections.push(Arc::clone(&connection));
							Ok(connection)
						}
						Err(e) => Err(e),
					}
				};
				// Wake anyone waiting at the cap: a connection appeared, or a
				// reserved slot was freed by a failed open.
				pool_ready().notify_all();
				return outcome;
			}
		}
	}
}

/// Number of pooled connections currently held for `url`'s server.
#[cfg(test)]
fn connection_count(url: &Url) -> Result<usize> {
	let key = ServerKey::from_url(url)?;
	let guard = pool().lock().map_err(|e| anyhow!("SFTP pool lock poisoned: {e}"))?;
	Ok(guard.get(&key).map_or(0, |server| server.connections.len()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_server_key_from_url_defaults() {
		let key = ServerKey::from_url(&Url::parse("sftp://host/path").unwrap()).unwrap();
		assert_eq!(key.host, "host");
		assert_eq!(key.port, 22);
		assert_eq!(key.username, "root");
	}

	#[test]
	fn test_server_key_from_url_explicit() {
		let key = ServerKey::from_url(&Url::parse("sftp://alice@host:2222/path").unwrap()).unwrap();
		assert_eq!(key.port, 2222);
		assert_eq!(key.username, "alice");
	}

	#[test]
	fn test_server_key_ignores_path() {
		let a = ServerKey::from_url(&Url::parse("sftp://host/one.bin").unwrap()).unwrap();
		let b = ServerKey::from_url(&Url::parse("sftp://host/two.bin").unwrap()).unwrap();
		assert_eq!(a, b);
	}

	#[test]
	fn test_server_key_missing_host() {
		assert!(ServerKey::from_url(&Url::parse("sftp:///path").unwrap()).is_err());
	}

	#[cfg(all(feature = "ssh2", unix))]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		#[tokio::test(flavor = "multi_thread")]
		async fn caps_connections_per_server() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"hello").await;
			let url = server.url("/a.bin");

			// Acquire more sources than the per-server cap, concurrently — the
			// surplus must reuse existing connections rather than open new
			// ones, even when many `acquire` calls race.
			let total = CONNECTIONS_PER_SERVER + 4;
			let mut handles = Vec::with_capacity(total);
			for _ in 0..total {
				let url = url.clone();
				handles.push(tokio::task::spawn_blocking(move || acquire(&url, None)));
			}
			let mut acquired = Vec::with_capacity(total);
			for handle in handles {
				acquired.push(handle.await.unwrap().unwrap());
			}

			assert_eq!(acquired.len(), total);
			assert_eq!(connection_count(&url).unwrap(), CONNECTIONS_PER_SERVER);
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn many_files_share_one_connection() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"aaaa").await;
			server.write_file("/b.bin", b"bbbbbb").await;
			let url = server.url("/a.bin");

			tokio::task::spawn_blocking(move || -> Result<()> {
				let connection = acquire(&url, None)?;
				let (id_a, size_a) = connection.register(Path::new("/a.bin"))?;
				let (id_b, size_b) = connection.register(Path::new("/b.bin"))?;
				assert_eq!(size_a, 4);
				assert_eq!(size_b, 6);
				assert_eq!(connection.read_range(id_a, &ByteRange::new(0, 4))?.as_slice(), b"aaaa");
				assert_eq!(connection.read_range(id_b, &ByteRange::new(2, 4))?.as_slice(), b"bbbb");
				connection.unregister(id_a);
				assert!(connection.read_range(id_a, &ByteRange::new(0, 4)).is_err());
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn reconnect_reopens_registered_files() {
			let server = TestSftpServer::start().await;
			server.write_file("/a.bin", b"hello").await;
			let url = server.url("/a.bin");

			tokio::task::spawn_blocking(move || -> Result<()> {
				let connection = acquire(&url, None)?;
				let (id, _) = connection.register(Path::new("/a.bin"))?;
				let generation = connection.generation()?;
				connection.reconnect(generation)?;
				assert_eq!(connection.generation()?, generation + 1);
				// The file handle survives the reconnect.
				assert_eq!(connection.read_range(id, &ByteRange::new(0, 5))?.as_slice(), b"hello");
				// A stale generation makes reconnect a no-op.
				connection.reconnect(generation)?;
				assert_eq!(connection.generation()?, generation + 1);
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
		}
	}
}
