//! In-process SFTP server backed by an in-memory filesystem, for integration tests.
#![cfg(all(feature = "sftp", any(test, feature = "test")))]

use reqwest::Url;
use russh::{
	Channel, ChannelId,
	keys::{
		Algorithm, PrivateKey, PublicKey,
		ssh_key::rand_core::{TryCryptoRng, TryRng},
	},
	server::{self, Auth, Msg, Session},
};

/// Test-only OS-backed RNG that satisfies `PrivateKey::random`'s `CryptoRng` bound.
///
/// rand_core 0.10 (used by russh's forked ssh-key) no longer re-exports `OsRng`, so
/// we provide a minimal adapter over `getrandom`. Implementing `TryRng<Error =
/// Infallible>` + `TryCryptoRng` is enough — rand_core's blanket impls give us
/// `Rng` + `CryptoRng` automatically.
///
/// `getrandom` rather than reading `/dev/urandom` directly: this module used to be
/// `cfg(unix)`, because `ssh2` confined SFTP to unix. It is not any more, and a
/// hard-coded device path fails on Windows with "The system cannot find the path
/// specified".
struct OsRng;

impl TryRng for OsRng {
	type Error = std::convert::Infallible;

	fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
		let mut b = [0u8; 4];
		self.try_fill_bytes(&mut b)?;
		Ok(u32::from_ne_bytes(b))
	}

	fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
		let mut b = [0u8; 8];
		self.try_fill_bytes(&mut b)?;
		Ok(u64::from_ne_bytes(b))
	}

	fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
		getrandom::fill(dst).expect("test OsRng: getrandom");
		Ok(())
	}
}

impl TryCryptoRng for OsRng {}
use std::{
	collections::{BTreeMap, HashMap, VecDeque},
	net::SocketAddr,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
	},
	time::Duration,
};

use russh_sftp::protocol::{
	Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, SftpHandle, Status, StatusCode, Version,
};
use tokio::{
	net::TcpListener,
	sync::{Mutex, oneshot},
	time,
};

/// The in-memory file system, by absolute path as [`normalize`] writes it.
///
/// Strings rather than `PathBuf`s: SFTP paths are `/`-separated on every
/// platform, and a `PathBuf` on Windows joins with `\`.
type Fs = Arc<Mutex<HashMap<String, FsEntry>>>;

/// The host keys of every test server started in this process.
///
/// Each server generates a fresh key and binds a random port, so recording them
/// in a known-hosts file is wrong twice over: the file fills up with entries for
/// servers that no longer exist, and once a port comes round again, the new key
/// is refused as a changed one.
static HOST_KEYS: std::sync::Mutex<Vec<PublicKey>> = std::sync::Mutex::new(Vec::new());

/// Whether `key` belongs to a test server, so host-key verification can let it
/// through without touching a known-hosts file.
pub(crate) fn is_test_server_key(key: &PublicKey) -> bool {
	HOST_KEYS
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner)
		.contains(key)
}

enum FsEntry {
	File(Vec<u8>),
	Dir,
	/// A symbolic link to the path it holds, as written: absolute, or relative
	/// to the folder the link is in.
	Symlink(String),
}

/// The modification time every entry reports: 2024-01-01T00:00:00Z.
const MTIME: u32 = 1_704_067_200;

/// How many entries one `readdir` reply carries at most.
///
/// Far fewer than a real server sends (OpenSSH sends 100), so that listing any
/// folder with a few entries takes several replies, and a client that reads only
/// the first one is caught.
const READDIR_BATCH: usize = 3;

/// How many links in a row are followed before a path counts as a loop.
const MAX_LINKS: usize = 8;

/// `path` as an absolute path with no `.`, `..`, repeated or trailing slashes.
///
/// A relative path is relative to `/`, the folder the server signs in to. `..`
/// at the root stays at the root, as on a POSIX file system.
fn normalize(path: &str) -> String {
	let mut parts: Vec<&str> = Vec::new();
	for part in path.split('/') {
		match part {
			"" | "." => {}
			".." => {
				parts.pop();
			}
			part => parts.push(part),
		}
	}
	format!("/{}", parts.join("/"))
}

/// The folder `path` is in; `None` for the root.
fn parent_of(path: &str) -> Option<&str> {
	match path.rsplit_once('/') {
		Some(("", "")) | None => None,
		Some(("", _)) => Some("/"),
		Some((parent, _)) => Some(parent),
	}
}

/// The first component of `path` below the folder `dir`, if `path` is below it.
fn child_of<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
	let rest = if dir == "/" {
		path.strip_prefix('/')?
	} else {
		path.strip_prefix(dir)?.strip_prefix('/')?
	};
	rest.split('/').next().filter(|name| !name.is_empty())
}

/// Whether `path` is a folder: the root, one made with `mkdir`, or one a file
/// or link below it implies.
fn is_dir(fs: &HashMap<String, FsEntry>, path: &str) -> bool {
	path == "/"
		|| matches!(fs.get(path), Some(FsEntry::Dir))
		|| (!fs.contains_key(path) && fs.keys().any(|key| child_of(key, path).is_some()))
}

/// Where the link at `link` points, as an absolute path.
fn link_target(link: &str, target: &str) -> String {
	if target.starts_with('/') {
		normalize(target)
	} else {
		normalize(&format!("{}/{target}", parent_of(link).unwrap_or("/")))
	}
}

/// `path` with links followed, as far as they go. Only the last component is
/// followed: a link in the middle of a path is not looked through.
fn resolve(fs: &HashMap<String, FsEntry>, path: &str) -> Result<String, StatusCode> {
	let mut path = path.to_string();
	for _ in 0..MAX_LINKS {
		match fs.get(&path) {
			Some(FsEntry::Symlink(target)) => path = link_target(&path, target),
			_ => return Ok(path),
		}
	}
	Err(StatusCode::Failure)
}

/// The attributes of what is at `path`, not following a link there.
fn attrs_of(fs: &HashMap<String, FsEntry>, path: &str) -> Result<FileAttributes, StatusCode> {
	let (size, permissions) = match fs.get(path) {
		Some(FsEntry::File(data)) => (data.len() as u64, 0o100_644),
		Some(FsEntry::Symlink(target)) => (target.len() as u64, 0o120_777),
		Some(FsEntry::Dir) => (0, 0o040_755),
		None if is_dir(fs, path) => (0, 0o040_755),
		None => return Err(StatusCode::NoSuchFile),
	};
	Ok(FileAttributes {
		size: Some(size),
		permissions: Some(permissions),
		atime: Some(MTIME),
		mtime: Some(MTIME),
		..Default::default()
	})
}

/// An `ls -l` line for a directory listing, which some clients show as is.
fn longname(name: &str, attrs: &FileAttributes) -> String {
	let (kind, mode) = match attrs.permissions.unwrap_or(0) & 0o170_000 {
		0o040_000 => ('d', "rwxr-xr-x"),
		0o120_000 => ('l', "rwxrwxrwx"),
		_ => ('-', "rw-r--r--"),
	};
	format!(
		"{kind}{mode} 1 testuser testuser {} Jan  1  2024 {name}",
		attrs.size.unwrap_or(0)
	)
}

/// An `SSH_FX_OK` status for `id`.
fn ok_status(id: u32) -> Status {
	Status {
		id,
		status_code: StatusCode::Ok,
		error_message: String::new(),
		language_tag: String::new(),
	}
}

// ---------------------------------------------------------------------------
// SFTP handler — error type is StatusCode (implements Into<StatusCode>)
// ---------------------------------------------------------------------------

/// What the reads of every session on a server have in common: how much they
/// sent, and how much one may send.
#[derive(Default)]
struct Reads {
	/// Bytes of file data sent in replies to reads.
	bytes_sent: AtomicU64,
	/// Replies to reads carry at most this many bytes. `0` is no limit.
	max_len: AtomicU32,
}

struct SftpHandler {
	fs: Fs,
	/// Open files, by handle.
	handles: HashMap<SftpHandle, String>,
	/// Open folders, by handle: the entries not sent yet.
	dir_handles: HashMap<SftpHandle, VecDeque<File>>,
	next_id: u64,
	drop_flag: Arc<AtomicBool>,
	reads: Arc<Reads>,
}

impl SftpHandler {
	fn new_handle(&mut self) -> SftpHandle {
		self.next_id += 1;
		SftpHandle::from(format!("h{}", self.next_id))
	}
}

// `async` with nothing to await in several methods: the trait declares them
// async, so the signatures are not ours to change.
#[expect(
	clippy::unused_async_trait_impl,
	reason = "the trait declares these `async`; this handler answers without awaiting"
)]
impl russh_sftp::server::Handler for SftpHandler {
	type Error = StatusCode;

	fn unimplemented(&self) -> Self::Error {
		StatusCode::OpUnsupported
	}

	async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
		Ok(Version::new())
	}

	/// SFTP v3 `SSH_FXP_RENAME`: fails when the target already exists. That is
	/// the whole reason `posix-rename@openssh.com` exists, so the restriction is
	/// modelled rather than glossed over.
	async fn rename(&mut self, id: u32, oldpath: String, newpath: String) -> Result<Status, Self::Error> {
		let mut fs = self.fs.lock().await;
		let (from, to) = (normalize(&oldpath), normalize(&newpath));
		if fs.contains_key(&to) {
			return Err(StatusCode::Failure);
		}
		let entry = fs.remove(&from).ok_or(StatusCode::NoSuchFile)?;
		fs.insert(to, entry);
		Ok(ok_status(id))
	}

	async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
		let mut fs = self.fs.lock().await;
		match fs.remove(&normalize(&filename)) {
			Some(_) => Ok(ok_status(id)),
			None => Err(StatusCode::NoSuchFile),
		}
	}

	async fn open(
		&mut self,
		id: u32,
		filename: String,
		pflags: OpenFlags,
		_attrs: FileAttributes,
	) -> Result<Handle, Self::Error> {
		let path = {
			let mut fs = self.fs.lock().await;
			let path = resolve(&fs, &normalize(&filename))?;

			if pflags.contains(OpenFlags::CREATE) {
				fs.entry(path.clone()).or_insert(FsEntry::File(Vec::new()));
			}
			if pflags.contains(OpenFlags::TRUNCATE)
				&& let Some(FsEntry::File(data)) = fs.get_mut(&path)
			{
				data.clear();
			}
			if !fs.contains_key(&path) {
				return Err(StatusCode::NoSuchFile);
			}
			path
		};

		let handle = self.new_handle();
		self.handles.insert(handle.clone(), path);
		Ok(Handle { id, handle })
	}

	async fn close(&mut self, id: u32, handle: SftpHandle) -> Result<Status, Self::Error> {
		self.handles.remove(&handle);
		self.dir_handles.remove(&handle);
		Ok(ok_status(id))
	}

	async fn read(&mut self, id: u32, handle: SftpHandle, offset: u64, len: u32) -> Result<Data, Self::Error> {
		if self.drop_flag.swap(false, Ordering::SeqCst) {
			return Err(StatusCode::BadMessage);
		}

		let path = self.handles.get(&handle).ok_or(StatusCode::BadMessage)?.clone();
		let fs = self.fs.lock().await;
		let Some(FsEntry::File(data)) = fs.get(&path) else {
			return Err(StatusCode::NoSuchFile);
		};

		let start = usize::try_from(offset).unwrap();
		if start >= data.len() {
			return Err(StatusCode::Eof);
		}
		let len = match self.reads.max_len.load(Ordering::SeqCst) {
			0 => len,
			max_len => len.min(max_len),
		};
		let end = (start + len as usize).min(data.len());
		self.reads.bytes_sent.fetch_add((end - start) as u64, Ordering::SeqCst);
		Ok(Data {
			id,
			data: data[start..end].to_vec(),
		})
	}

	async fn write(&mut self, id: u32, handle: SftpHandle, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
		if self.drop_flag.swap(false, Ordering::SeqCst) {
			return Err(StatusCode::BadMessage);
		}

		let path = self.handles.get(&handle).ok_or(StatusCode::BadMessage)?.clone();
		let mut fs = self.fs.lock().await;
		match fs.get_mut(&path) {
			Some(FsEntry::File(file_data)) => {
				let pos = usize::try_from(offset).unwrap();
				let end = pos + data.len();
				if end > file_data.len() {
					file_data.resize(end, 0);
				}
				file_data[pos..end].copy_from_slice(&data);
			}
			_ => return Err(StatusCode::NoSuchFile),
		}
		Ok(ok_status(id))
	}

	/// Follows a link at `path`, as `stat(2)` does: a link to nothing is no
	/// such file.
	async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
		let fs = self.fs.lock().await;
		let path = resolve(&fs, &normalize(&path))?;
		Ok(Attrs {
			id,
			attrs: attrs_of(&fs, &path)?,
		})
	}

	/// Describes a link at `path` itself, as `lstat(2)` does.
	async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
		let fs = self.fs.lock().await;
		Ok(Attrs {
			id,
			attrs: attrs_of(&fs, &normalize(&path))?,
		})
	}

	async fn fstat(&mut self, id: u32, handle: SftpHandle) -> Result<Attrs, Self::Error> {
		let path = self.handles.get(&handle).ok_or(StatusCode::BadMessage)?.clone();
		self.stat(id, path).await
	}

	async fn mkdir(&mut self, id: u32, path: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
		let mut fs = self.fs.lock().await;
		fs.entry(normalize(&path)).or_insert(FsEntry::Dir);
		Ok(ok_status(id))
	}

	/// Lists the folder at `path`, following a link there, with the entries
	/// directly below it: what was written or made there, and the folders the
	/// paths of deeper entries imply. `.` and `..` come first, as OpenSSH sends
	/// them.
	async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
		let entries = {
			let fs = self.fs.lock().await;
			let dir = resolve(&fs, &normalize(&path))?;
			if !is_dir(&fs, &dir) {
				return Err(if fs.contains_key(&dir) {
					StatusCode::Failure
				} else {
					StatusCode::NoSuchFile
				});
			}

			let mut names: BTreeMap<String, FileAttributes> = BTreeMap::new();
			for key in fs.keys() {
				if let Some(name) = child_of(key, &dir)
					&& !names.contains_key(name)
				{
					let child = if dir == "/" {
						format!("/{name}")
					} else {
						format!("{dir}/{name}")
					};
					names.insert(name.to_string(), attrs_of(&fs, &child)?);
				}
			}

			let mut entries: VecDeque<File> = VecDeque::new();
			for (name, path) in [(".", dir.as_str()), ("..", parent_of(&dir).unwrap_or("/"))] {
				let attrs = attrs_of(&fs, path)?;
				entries.push_back(File {
					filename: name.to_string(),
					longname: longname(name, &attrs),
					attrs,
				});
			}
			entries.extend(names.into_iter().map(|(name, attrs)| File {
				longname: longname(&name, &attrs),
				filename: name,
				attrs,
			}));
			entries
		};

		let handle = self.new_handle();
		self.dir_handles.insert(handle.clone(), entries);
		Ok(Handle { id, handle })
	}

	/// Up to [`READDIR_BATCH`] entries at a time, then `EOF`.
	async fn readdir(&mut self, id: u32, handle: SftpHandle) -> Result<Name, Self::Error> {
		let entries = self.dir_handles.get_mut(&handle).ok_or(StatusCode::BadMessage)?;
		if entries.is_empty() {
			return Err(StatusCode::Eof);
		}
		let count = entries.len().min(READDIR_BATCH);
		Ok(Name {
			id,
			files: entries.drain(..count).collect(),
		})
	}

	/// `path` as an absolute path, relative ones resolved against `/`, with a
	/// link at the end followed. Unlike OpenSSH, a path to nothing is not an
	/// error: it comes back normalised.
	async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
		let fs = self.fs.lock().await;
		let path = resolve(&fs, &normalize(&path))?;
		Ok(Name {
			id,
			files: vec![File::dummy(path)],
		})
	}

	async fn readlink(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
		let fs = self.fs.lock().await;
		match fs.get(&normalize(&path)) {
			Some(FsEntry::Symlink(target)) => Ok(Name {
				id,
				files: vec![File::dummy(target.clone())],
			}),
			Some(_) => Err(StatusCode::Failure),
			None => Err(StatusCode::NoSuchFile),
		}
	}
}

// ---------------------------------------------------------------------------
// SSH handler
// ---------------------------------------------------------------------------

struct SshHandler {
	fs: Fs,
	drop_flag: Arc<AtomicBool>,
	reads: Arc<Reads>,
	channel: Option<Channel<Msg>>,
}

// `async` with nothing to await in several methods: the trait declares them
// async, so the signatures are not ours to change.
#[expect(
	clippy::unused_async_trait_impl,
	reason = "the trait declares these `async`; this handler answers without awaiting"
)]
impl server::Handler for SshHandler {
	type Error = anyhow::Error;

	async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
		if user == "testuser" && password == "testpass" {
			Ok(Auth::Accept)
		} else {
			Ok(Auth::Reject {
				proceed_with_methods: None,
				partial_success: false,
			})
		}
	}

	async fn channel_open_session(
		&mut self,
		channel: Channel<Msg>,
		reply: russh::server::ChannelOpenHandle,
		_session: &mut Session,
	) -> Result<(), Self::Error> {
		self.channel = Some(channel);
		reply.accept().await;
		Ok(())
	}

	async fn subsystem_request(
		&mut self,
		channel_id: ChannelId,
		name: &str,
		session: &mut Session,
	) -> Result<(), Self::Error> {
		if name == "sftp" {
			let _ = session.channel_success(channel_id);
			if let Some(channel) = self.channel.take() {
				let sftp_handler = SftpHandler {
					fs: self.fs.clone(),
					handles: HashMap::new(),
					dir_handles: HashMap::new(),
					next_id: 0,
					drop_flag: self.drop_flag.clone(),
					reads: self.reads.clone(),
				};
				tokio::spawn(async move {
					russh_sftp::server::run(channel.into_stream(), sftp_handler).await;
				});
			}
		}
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// TestSftpServer
// ---------------------------------------------------------------------------

/// An in-process SFTP server backed by an in-memory filesystem.
///
/// Used by integration tests in the surrounding modules to exercise
/// the real SFTP client code against a controlled server.
pub struct TestSftpServer {
	addr: SocketAddr,
	fs: Fs,
	drop_flag: Arc<AtomicBool>,
	reads: Arc<Reads>,
	/// Signals the server thread to exit its accept loop and shut down.
	shutdown: Arc<AtomicBool>,
	/// SFTP operation timeout (ms) embedded in URLs returned by [`Self::url`].
	/// Defaults to 2000 ms in normal builds; automatically raised to 5000 ms
	/// under `cargo-llvm-cov` (which sets `cfg(coverage)`) because the
	/// instrumentation overhead slows the in-process server enough that the SSH
	/// handshake can exceed 500 ms.
	timeout_ms: u32,
}

impl Drop for TestSftpServer {
	fn drop(&mut self) {
		self.shutdown.store(true, Ordering::SeqCst);
		// The server thread polls this flag every 100 ms and exits on its own;
		// we do not join it to avoid blocking the async test context.
	}
}

impl TestSftpServer {
	/// Bind to a random localhost port and start accepting SSH connections.
	///
	/// The server runs on a **dedicated two-thread Tokio runtime** in its own OS
	/// thread, completely isolated from the calling test's runtime. This means the
	/// server's async tasks (russh handshake, SFTP protocol) always have CPU
	/// available regardless of how many tests run in parallel, eliminating the
	/// SFTP timeout failures that occurred when the server shared a
	/// runtime with heavily-loaded test workers.
	pub async fn start() -> Self {
		let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap();
		HOST_KEYS
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.push(key.public_key().clone());
		let config = Arc::new(server::Config {
			keys: vec![key],
			..Default::default()
		});

		let fs: Fs = Arc::new(Mutex::new(HashMap::new()));
		let drop_flag = Arc::new(AtomicBool::new(false));
		let shutdown = Arc::new(AtomicBool::new(false));

		// Bind synchronously so we know the port before spawning the server thread.
		let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
		std_listener.set_nonblocking(true).unwrap();
		let addr = std_listener.local_addr().unwrap();

		let fs_clone = fs.clone();
		let drop_flag_clone = drop_flag.clone();
		let reads = Arc::new(Reads::default());
		let reads_clone = reads.clone();
		let shutdown_clone = shutdown.clone();

		let (ready_tx, ready_rx) = oneshot::channel::<()>();

		std::thread::spawn(move || {
			let rt = tokio::runtime::Builder::new_multi_thread()
				.worker_threads(2)
				.enable_all()
				.build()
				.unwrap();
			rt.block_on(async move {
				let listener = TcpListener::from_std(std_listener).unwrap();
				// Signal to the caller that the listener is bound and accepting.
				let _ = ready_tx.send(());
				loop {
					if shutdown_clone.load(Ordering::SeqCst) {
						break;
					}
					match time::timeout(Duration::from_millis(100), listener.accept()).await {
						Ok(Ok((stream, _))) => {
							let handler = SshHandler {
								fs: fs_clone.clone(),
								drop_flag: drop_flag_clone.clone(),
								reads: reads_clone.clone(),
								channel: None,
							};
							let config = config.clone();
							tokio::spawn(async move {
								let _ = server::run_stream(config, stream, handler).await;
							});
						}
						Ok(Err(_)) => break,
						Err(_) => {} // 100 ms poll interval — loop back to check shutdown
					}
				}
			});
		});

		// Block until the server's runtime has started and is accepting connections.
		ready_rx.await.expect("SFTP test server panicked during startup");

		// Coverage builds instrument every instruction and are significantly slower;
		// give them extra budget. Normal builds use 2000 ms — enough headroom above
		// the ~350 ms observed CI handshake time to absorb OS scheduling jitter.
		#[cfg(coverage)]
		let timeout_ms = 5000u32;
		#[cfg(not(coverage))]
		let timeout_ms = 2000u32;

		TestSftpServer {
			addr,
			fs,
			drop_flag,
			reads,
			shutdown,
			timeout_ms,
		}
	}

	/// Returns `sftp://testuser:testpass@127.0.0.1:{port}{path}?timeout_ms={timeout_ms}`.
	/// The `timeout_ms` query parameter is read by `sftp_utils::open_session` in
	/// test builds to set the SFTP operation timeout for that connection.
	#[must_use]
	pub fn url(&self, path: &str) -> Url {
		Url::parse(&format!(
			"sftp://testuser:testpass@127.0.0.1:{}{}?timeout_ms={}",
			self.addr.port(),
			path,
			self.timeout_ms,
		))
		.unwrap()
	}

	/// Read a file from the in-memory filesystem (assert writes).
	pub async fn read_file(&self, path: &str) -> Vec<u8> {
		let fs = self.fs.lock().await;
		match fs.get(&normalize(path)) {
			Some(FsEntry::File(data)) => data.clone(),
			_ => Vec::new(),
		}
	}

	/// Seed a file into the in-memory filesystem (set up reads).
	///
	/// The folders above it exist from then on, listed and reported as
	/// folders, without being made.
	pub async fn write_file(&self, path: &str, data: &[u8]) {
		let mut fs = self.fs.lock().await;
		fs.insert(normalize(path), FsEntry::File(data.to_vec()));
	}

	/// Seed an empty folder, as `mkdir` makes one.
	pub async fn create_dir(&self, path: &str) {
		let mut fs = self.fs.lock().await;
		fs.insert(normalize(path), FsEntry::Dir);
	}

	/// Seed a symbolic link at `link` to `target`, which may be absolute or
	/// relative to the folder of `link`, and need not exist.
	///
	/// `stat`, `open` and `opendir` follow it, `lstat` and `readlink` describe
	/// the link itself, and a listing shows it as a link. Only a link at the end
	/// of a path is followed, not one in the middle of it.
	pub async fn create_symlink(&self, link: &str, target: &str) {
		let mut fs = self.fs.lock().await;
		fs.insert(normalize(link), FsEntry::Symlink(target.to_string()));
	}

	/// Bytes of file data the server has sent in replies to reads, over all
	/// sessions.
	#[must_use]
	pub fn bytes_read(&self) -> u64 {
		self.reads.bytes_sent.load(Ordering::SeqCst)
	}

	/// Answer every read with at most `len` bytes, as servers do that cap
	/// their reads without saying so.
	pub fn set_max_read_len(&self, len: u32) {
		self.reads.max_len.store(len, Ordering::SeqCst);
	}

	/// Cause the next `read` or `write` SFTP operation to fail,
	/// exercising the retry/reconnect path in the client.
	pub fn schedule_disconnect(&self) {
		self.drop_flag.store(true, Ordering::SeqCst);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::io::sftp_utils::{Sftp, open_session, open_sftp};

	/// A session on `server`, opened as a downstream application opens one.
	async fn session(server: &TestSftpServer) -> (crate::io::sftp_utils::SshHandle, Sftp) {
		let handle = open_session(&server.url("/"), None).await.unwrap();
		let sftp = open_sftp(&handle).await.unwrap();
		(handle, sftp)
	}

	/// What `read_dir` lists in `path`: names, with `d`, `l` or `-` and a size.
	async fn listing(sftp: &Sftp, path: &str) -> Vec<String> {
		let mut entries: Vec<String> = sftp
			.read_dir(path)
			.await
			.unwrap()
			.map(|entry| {
				let metadata = entry.metadata();
				let kind = if metadata.is_dir() {
					'd'
				} else if metadata.is_symlink() {
					'l'
				} else {
					'-'
				};
				format!("{kind} {} {}", entry.file_name(), metadata.len())
			})
			.collect();
		entries.sort();
		entries
	}

	#[test]
	fn paths_are_normalised_against_the_root() {
		for (path, expected) in [
			(".", "/"),
			("", "/"),
			("/", "/"),
			("a/../b", "/b"),
			("/a/./b//c/", "/a/b/c"),
			("../..", "/"),
			("/a/b/../../../c", "/c"),
		] {
			assert_eq!(normalize(path), expected, "{path:?}");
		}
		assert_eq!(parent_of("/"), None);
		assert_eq!(parent_of("/a"), Some("/"));
		assert_eq!(parent_of("/a/b"), Some("/a"));
	}

	#[tokio::test]
	async fn read_dir_lists_files_and_folders_once_each() {
		let server = TestSftpServer::start().await;
		server.write_file("/data/a.versatiles", b"12345").await;
		server.write_file("/data/b.pmtiles", b"123").await;
		// Two files under one implied folder: listed once.
		server.write_file("/data/sub/c.versatiles", b"1").await;
		server.write_file("/data/sub/deeper/d.versatiles", b"1").await;
		server.create_dir("/data/made").await;
		server.write_file("/elsewhere/e.versatiles", b"1").await;
		let (_handle, sftp) = session(&server).await;

		// Five entries plus `.` and `..` take three replies of at most three.
		assert_eq!(
			listing(&sftp, "/data").await,
			["- a.versatiles 5", "- b.pmtiles 3", "d made 0", "d sub 0"]
		);
		assert_eq!(listing(&sftp, "/").await, ["d data 0", "d elsewhere 0"]);
		assert_eq!(listing(&sftp, "/data/sub").await, ["- c.versatiles 1", "d deeper 0"]);
		// A trailing slash or a relative path names the same folder.
		assert_eq!(listing(&sftp, "data/sub/").await, listing(&sftp, "/data/sub").await);
	}

	#[tokio::test]
	async fn an_empty_folder_lists_nothing_and_a_missing_one_is_an_error() {
		let server = TestSftpServer::start().await;
		server.create_dir("/empty").await;
		server.write_file("/file.bin", b"x").await;
		let (_handle, sftp) = session(&server).await;

		assert_eq!(listing(&sftp, "/empty").await, Vec::<String>::new());
		assert!(sftp.read_dir("/missing").await.is_err());
		assert!(sftp.read_dir("/file.bin").await.is_err());
	}

	#[tokio::test]
	async fn folders_are_reported_as_folders() {
		let server = TestSftpServer::start().await;
		server.write_file("/data/sub/a.bin", b"abc").await;
		let (_handle, sftp) = session(&server).await;

		for folder in ["/", "/data", "/data/sub"] {
			assert!(sftp.metadata(folder).await.unwrap().is_dir(), "{folder}");
		}
		let file = sftp.metadata("/data/sub/a.bin").await.unwrap();
		assert!(file.is_regular() && file.len() == 3);
		assert!(file.modified().is_ok());
		assert!(sftp.metadata("/data/nothing").await.is_err());
	}

	#[tokio::test]
	async fn canonicalize_resolves_against_the_root() {
		let server = TestSftpServer::start().await;
		let (_handle, sftp) = session(&server).await;

		assert_eq!(sftp.canonicalize(".").await.unwrap(), "/");
		assert_eq!(sftp.canonicalize("a/../b").await.unwrap(), "/b");
		assert_eq!(sftp.canonicalize("/a//b/./c/").await.unwrap(), "/a/b/c");
	}

	#[tokio::test]
	async fn links_are_followed_by_stat_and_described_by_lstat() {
		let server = TestSftpServer::start().await;
		server.write_file("/real/folder/tiles.versatiles", b"tiles").await;
		server.create_symlink("/links/to-folder", "../real/folder").await;
		server
			.create_symlink("/links/to-file", "/real/folder/tiles.versatiles")
			.await;
		server.create_symlink("/links/to-nothing", "/gone").await;
		server.create_symlink("/links/loop", "/links/loop").await;
		let (_handle, sftp) = session(&server).await;

		assert_eq!(
			listing(&sftp, "/links").await,
			["l loop 11", "l to-file 29", "l to-folder 14", "l to-nothing 5"]
		);

		// Following a link to a folder, as a file dialog does.
		assert!(sftp.metadata("/links/to-folder").await.unwrap().is_dir());
		assert_eq!(listing(&sftp, "/links/to-folder").await, ["- tiles.versatiles 5"]);
		assert_eq!(sftp.canonicalize("/links/to-folder").await.unwrap(), "/real/folder");

		// Following a link to a file, and reading through it.
		assert_eq!(sftp.metadata("/links/to-file").await.unwrap().len(), 5);
		assert_eq!(sftp.read("/links/to-file").await.unwrap(), b"tiles");

		// A link to nothing, or to itself, is there, but leads nowhere.
		for link in ["/links/to-nothing", "/links/loop"] {
			assert!(sftp.symlink_metadata(link).await.unwrap().is_symlink(), "{link}");
			assert!(sftp.metadata(link).await.is_err(), "{link}");
		}

		assert_eq!(sftp.read_link("/links/to-folder").await.unwrap(), "../real/folder");
		assert!(sftp.read_link("/real/folder").await.is_err());
	}
}
