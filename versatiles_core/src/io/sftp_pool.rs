//! Connection pool for SFTP sources.
//!
//! Opening one full SSH connection per source does not scale: a burst of
//! parallel opens to the same server trips `sshd`'s `MaxStartups` limit and
//! the server rejects connections. This module keeps a small, fixed number
//! of SSH connections per server and lets many file handles share each one
//! (SFTP multiplexes file handles over a single channel), so the number of
//! sources is decoupled from the number of TCP/SSH connections.

use super::sftp_utils;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, anyhow};
use reqwest::Url;
use ssh2::{Session, Sftp};
use std::{
	collections::HashMap,
	io::{Read, Seek, SeekFrom},
	path::{Path, PathBuf},
	sync::{Arc, Mutex, MutexGuard, OnceLock},
};

/// SSH connections kept open per server. Each connection carries one SFTP
/// channel that holds many file handles, so this only bounds I/O concurrency
/// and lock contention — not how many sources you can open.
const CONNECTIONS_PER_SERVER: usize = 4;

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

/// The connections held for one server, with a round-robin cursor.
#[derive(Default)]
struct ServerPool {
	connections: Vec<Arc<Connection>>,
	next: usize,
}

static POOL: OnceLock<Mutex<HashMap<ServerKey, ServerPool>>> = OnceLock::new();

/// Get a pooled connection for `url`.
///
/// New connections are created until the server reaches
/// [`CONNECTIONS_PER_SERVER`], after which existing connections are handed
/// out round-robin. The global pool lock is intentionally held across
/// connection setup: this serializes the connect burst and keeps `sshd`'s
/// `MaxStartups` limit from rejecting parallel opens.
pub fn acquire(url: &Url, identity_file: Option<&Path>) -> Result<Arc<Connection>> {
	let key = ServerKey::from_url(url)?;
	let mut pool = POOL
		.get_or_init(|| Mutex::new(HashMap::new()))
		.lock()
		.map_err(|e| anyhow!("SFTP pool lock poisoned: {e}"))?;
	let server = pool.entry(key).or_default();

	if server.connections.len() < CONNECTIONS_PER_SERVER {
		let connection = Connection::open(url, identity_file)?;
		server.connections.push(Arc::clone(&connection));
		Ok(connection)
	} else {
		let connection = Arc::clone(&server.connections[server.next]);
		server.next = (server.next + 1) % server.connections.len();
		Ok(connection)
	}
}

/// Number of pooled connections currently held for `url`'s server.
#[cfg(test)]
fn connection_count(url: &Url) -> Result<usize> {
	let key = ServerKey::from_url(url)?;
	let pool = POOL
		.get_or_init(|| Mutex::new(HashMap::new()))
		.lock()
		.map_err(|e| anyhow!("SFTP pool lock poisoned: {e}"))?;
	Ok(pool.get(&key).map_or(0, |server| server.connections.len()))
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

			// Acquire far more connections than the per-server cap.
			let acquired = tokio::task::spawn_blocking({
				let url = url.clone();
				move || -> Result<Vec<Arc<Connection>>> { (0..12).map(|_| acquire(&url, None)).collect() }
			})
			.await
			.unwrap()
			.unwrap();

			assert_eq!(acquired.len(), 12);
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
