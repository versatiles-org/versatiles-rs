//! In-process SFTP server backed by an in-memory filesystem, for integration tests.
#![cfg(all(feature = "ssh2", test, unix))]

use reqwest::Url;
use russh::{
	Channel, ChannelId,
	keys::{
		Algorithm, PrivateKey,
		ssh_key::rand_core::{TryCryptoRng, TryRng},
	},
	server::{self, Auth, Msg, Session},
};

/// Test-only OS-backed RNG that satisfies `PrivateKey::random`'s `CryptoRng` bound.
///
/// rand_core 0.10 (used by russh's forked ssh-key) no longer re-exports `OsRng`, so
/// we provide a minimal `/dev/urandom`-backed adapter. Implementing `TryRng<Error =
/// Infallible>` + `TryCryptoRng` is enough — rand_core's blanket impls give us
/// `Rng` + `CryptoRng` automatically.
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
		use std::io::Read;
		let mut f = std::fs::File::open("/dev/urandom").expect("test OsRng: open /dev/urandom");
		f.read_exact(dst).expect("test OsRng: read /dev/urandom");
		Ok(())
	}
}

impl TryCryptoRng for OsRng {}
use russh_sftp::protocol::{Attrs, Data, FileAttributes, Handle, OpenFlags, Status, StatusCode, Version};
use std::{
	collections::HashMap,
	net::SocketAddr,
	path::PathBuf,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

type Fs = Arc<Mutex<HashMap<PathBuf, FsEntry>>>;

enum FsEntry {
	File(Vec<u8>),
	Dir,
}

// ---------------------------------------------------------------------------
// SFTP handler — error type is StatusCode (implements Into<StatusCode>)
// ---------------------------------------------------------------------------

struct SftpHandler {
	fs: Fs,
	handles: HashMap<String, PathBuf>,
	next_id: u64,
	drop_flag: Arc<AtomicBool>,
}

impl russh_sftp::server::Handler for SftpHandler {
	type Error = StatusCode;

	fn unimplemented(&self) -> Self::Error {
		StatusCode::OpUnsupported
	}

	async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
		Ok(Version::new())
	}

	async fn open(
		&mut self,
		id: u32,
		filename: String,
		pflags: OpenFlags,
		_attrs: FileAttributes,
	) -> Result<Handle, Self::Error> {
		let path = PathBuf::from(&filename);
		let mut fs = self.fs.lock().await;

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

		self.next_id += 1;
		let handle = format!("h{}", self.next_id);
		self.handles.insert(handle.clone(), path);
		Ok(Handle { id, handle })
	}

	async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
		self.handles.remove(&handle);
		Ok(Status {
			id,
			status_code: StatusCode::Ok,
			error_message: String::new(),
			language_tag: String::new(),
		})
	}

	async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
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
		let end = (start + len as usize).min(data.len());
		Ok(Data {
			id,
			data: data[start..end].to_vec(),
		})
	}

	async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
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
		Ok(Status {
			id,
			status_code: StatusCode::Ok,
			error_message: String::new(),
			language_tag: String::new(),
		})
	}

	async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
		let path = PathBuf::from(&path);
		let fs = self.fs.lock().await;
		let attrs = match fs.get(&path) {
			Some(FsEntry::File(d)) => FileAttributes {
				size: Some(d.len() as u64),
				..Default::default()
			},
			Some(FsEntry::Dir) => FileAttributes {
				size: Some(0),
				..Default::default()
			},
			None => return Err(StatusCode::NoSuchFile),
		};
		Ok(Attrs { id, attrs })
	}

	async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
		self.stat(id, path).await
	}

	async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
		let path = self.handles.get(&handle).ok_or(StatusCode::BadMessage)?.clone();
		self.stat(id, path.to_string_lossy().into_owned()).await
	}

	async fn mkdir(&mut self, id: u32, path: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
		let path = PathBuf::from(&path);
		let mut fs = self.fs.lock().await;
		fs.entry(path).or_insert(FsEntry::Dir);
		Ok(Status {
			id,
			status_code: StatusCode::Ok,
			error_message: String::new(),
			language_tag: String::new(),
		})
	}
}

// ---------------------------------------------------------------------------
// SSH handler
// ---------------------------------------------------------------------------

struct SshHandler {
	fs: Fs,
	drop_flag: Arc<AtomicBool>,
	channel: Option<Channel<Msg>>,
}

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
		_session: &mut Session,
	) -> Result<bool, Self::Error> {
		self.channel = Some(channel);
		Ok(true)
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
					next_id: 0,
					drop_flag: self.drop_flag.clone(),
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
	_handle: JoinHandle<()>,
}

impl TestSftpServer {
	/// Bind to a random localhost port and start accepting SSH connections.
	pub async fn start() -> Self {
		let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap();
		let config = Arc::new(server::Config {
			keys: vec![key],
			..Default::default()
		});

		let fs: Fs = Arc::new(Mutex::new(HashMap::new()));
		let drop_flag = Arc::new(AtomicBool::new(false));

		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();

		let fs_clone = fs.clone();
		let drop_flag_clone = drop_flag.clone();

		let handle = tokio::spawn(async move {
			loop {
				let Ok((stream, _)) = listener.accept().await else {
					break;
				};
				let handler = SshHandler {
					fs: fs_clone.clone(),
					drop_flag: drop_flag_clone.clone(),
					channel: None,
				};
				let config = config.clone();
				tokio::spawn(async move {
					let _ = server::run_stream(config, stream, handler).await;
				});
			}
		});

		TestSftpServer {
			addr,
			fs,
			drop_flag,
			_handle: handle,
		}
	}

	/// Returns `sftp://testuser:testpass@127.0.0.1:{port}{path}`.
	pub fn url(&self, path: &str) -> Url {
		Url::parse(&format!(
			"sftp://testuser:testpass@127.0.0.1:{}{}",
			self.addr.port(),
			path
		))
		.unwrap()
	}

	/// Read a file from the in-memory filesystem (assert writes).
	pub async fn read_file(&self, path: &str) -> Vec<u8> {
		let fs = self.fs.lock().await;
		match fs.get(&PathBuf::from(path)) {
			Some(FsEntry::File(data)) => data.clone(),
			_ => Vec::new(),
		}
	}

	/// Seed a file into the in-memory filesystem (set up reads).
	pub async fn write_file(&self, path: &str, data: &[u8]) {
		let mut fs = self.fs.lock().await;
		fs.insert(PathBuf::from(path), FsEntry::File(data.to_vec()));
	}

	/// Cause the next `read` or `write` SFTP operation to fail,
	/// exercising the retry/reconnect path in the client.
	pub fn schedule_disconnect(&self) {
		self.drop_flag.store(true, Ordering::SeqCst);
	}
}
