//! High-level SFTP wrappers that hide `ssh2` types from downstream crates.
//!
//! These types allow `versatiles_container` to use SFTP functionality without
//! depending on the `ssh2` crate directly.

use super::sftp_utils;
use anyhow::{Context, Result, bail};
use reqwest::Url;
use std::{
	io::Write,
	path::{Path, PathBuf},
	thread,
	time::Duration,
};

const MAX_RETRIES: u32 = 2;

/// A [`Write`] stream to a remote file via SFTP.
///
/// Keeps the SSH session alive for the lifetime of the writer.
pub struct SftpWriteStream {
	file: ssh2::File,
	_session: ssh2::Session,
}

// ssh2::Session and ssh2::File are backed by libssh2 ref-counted handles.
unsafe impl Send for SftpWriteStream {}

impl SftpWriteStream {
	/// Open a remote file for writing from an SFTP URL.
	pub fn from_url(url: &Url, identity_file: Option<&Path>) -> Result<Self> {
		let session = sftp_utils::open_session(url, identity_file)?;
		let remote_path = sftp_utils::remote_path(url);
		let sftp = session.sftp()?;
		let file = sftp
			.create(&remote_path)
			.with_context(|| format!("failed to create remote file {remote_path:?}"))?;

		Ok(Self {
			file,
			_session: session,
		})
	}
}

impl Write for SftpWriteStream {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.file.write(buf)
	}
	fn flush(&mut self) -> std::io::Result<()> {
		self.file.flush()
	}
}

/// A remote filesystem via SFTP for directory-style tile operations.
///
/// Provides `mkdir_p` and `write_file` without exposing `ssh2` types.
pub struct SftpFileSystem {
	sftp: ssh2::Sftp,
	base_path: PathBuf,
	url: Url,
	identity_file: Option<PathBuf>,
	_session: ssh2::Session,
}

// ssh2::Sftp is backed by libssh2 ref-counted handles.
unsafe impl Send for SftpFileSystem {}
unsafe impl Sync for SftpFileSystem {}

impl SftpFileSystem {
	/// Connect to an SFTP server and use the URL path as the base directory.
	pub fn from_url(url: &Url, identity_file: Option<&Path>) -> Result<Self> {
		let session = sftp_utils::open_session(url, identity_file)?;
		let base_path = sftp_utils::remote_path(url);
		let sftp = session.sftp()?;

		// Create base directory (ignore error if it already exists)
		if let Err(e) = sftp.mkdir(&base_path, 0o755) {
			match sftp.stat(&base_path) {
				Ok(stat) if stat.is_dir() => {}
				_ => return Err(e).with_context(|| format!("failed to create base directory {base_path:?}")),
			}
		}

		Ok(Self {
			sftp,
			base_path,
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
			_session: session,
		})
	}

	/// Reconnect the SFTP session (e.g. after a network error).
	fn reconnect(&mut self) -> Result<()> {
		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref())?;
		let sftp = session.sftp()?;
		self.sftp = sftp;
		self._session = session;
		Ok(())
	}

	/// Write a file at `rel_path` relative to the base directory.
	///
	/// Creates parent directories as needed. Retries on error since
	/// `write_file` is idempotent (creates/overwrites a complete file).
	pub fn write_file(&mut self, rel_path: &str, data: &[u8]) -> Result<()> {
		let full_path = self.base_path.join(rel_path);

		// Create parent directories
		if let Some(parent) = full_path.parent() {
			self.mkdir_p(parent)?;
		}

		let total_attempts = MAX_RETRIES + 1;

		for attempt in 0..=MAX_RETRIES {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = Duration::from_secs(1 << (attempt - 1));
				log::warn!("SFTP write file {full_path:?}: retrying ({attempt_label}, waiting {backoff:?})");
				thread::sleep(backoff);
				if let Err(e) = self.reconnect() {
					log::warn!("SFTP write file {full_path:?}: reconnect failed: {e} ({attempt_label})");
					if attempt >= MAX_RETRIES {
						return Err(e).with_context(|| {
							format!("could not write file {full_path:?} — reconnect failed after {total_attempts} attempts")
						});
					}
					continue;
				}
			}

			match self.try_write_file(&full_path, data) {
				Ok(()) => return Ok(()),
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!("SFTP write file {full_path:?}: {e} ({attempt_label}), will retry");
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!("could not write file {full_path:?} — gave up after {total_attempts} attempts")
					});
				}
			}
		}

		unreachable!()
	}

	fn try_write_file(&self, full_path: &Path, data: &[u8]) -> Result<()> {
		let mut file = self
			.sftp
			.create(full_path)
			.with_context(|| format!("failed to create remote file {full_path:?}"))?;
		file.write_all(data)?;
		Ok(())
	}

	/// Recursively create directories.
	///
	/// Returns an error if a path component cannot be created and does not
	/// already exist as a directory.
	fn mkdir_p(&self, path: &Path) -> Result<()> {
		if let Some(parent) = path.parent() {
			self.mkdir_p(parent)?;
		}
		if let Err(e) = self.sftp.mkdir(path, 0o755) {
			// Check whether the directory already exists
			match self.sftp.stat(path) {
				Ok(stat) if stat.is_dir() => {}
				_ => bail!("failed to create directory {path:?}: {e}"),
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_sftp_write_stream_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = SftpWriteStream::from_url(&url, None);
		assert!(result.is_err());
	}

	#[test]
	fn test_sftp_write_stream_with_identity_file() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = SftpWriteStream::from_url(&url, Some(Path::new("/nonexistent/key")));
		assert!(result.is_err());
	}

	#[test]
	fn test_sftp_filesystem_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/").unwrap();
		let result = SftpFileSystem::from_url(&url, None);
		assert!(result.is_err());
	}

	#[cfg(feature = "ssh2")]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		// --- SftpWriteStream ---

		#[tokio::test(flavor = "multi_thread")]
		async fn write_stream_write_and_flush() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut stream = SftpWriteStream::from_url(&url, None)?;
				stream.write_all(b"hello world")?;
				stream.flush()?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"hello world");
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn write_stream_multiple_writes() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut stream = SftpWriteStream::from_url(&url, None)?;
				stream.write_all(b"foo")?;
				stream.write_all(b"bar")?;
				stream.write_all(b"baz")?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"foobarbaz");
		}

		// --- SftpFileSystem ---

		#[tokio::test(flavor = "multi_thread")]
		async fn write_file_creates_file() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut fs = SftpFileSystem::from_url(&url, None)?;
				fs.write_file("a.bin", b"test data")?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/base/a.bin").await, b"test data");
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn write_file_creates_parent_dirs() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut fs = SftpFileSystem::from_url(&url, None)?;
				fs.write_file("a/b/c.bin", b"nested")?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/base/a/b/c.bin").await, b"nested");
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn write_file_overwrites_existing() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
				let mut fs = SftpFileSystem::from_url(&url, None)?;
				fs.write_file("a.bin", b"first")?;
				fs.write_file("a.bin", b"second")?;
				Ok(())
			})
			.await
			.unwrap()
			.unwrap();
			assert_eq!(server.read_file("/base/a.bin").await, b"second");
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn write_file_retry_after_disconnect() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			let mut fs = tokio::task::spawn_blocking(move || SftpFileSystem::from_url(&url, None))
				.await
				.unwrap()
				.unwrap();
			server.schedule_disconnect();
			tokio::task::spawn_blocking(move || fs.write_file("retry.bin", b"retried"))
				.await
				.unwrap()
				.unwrap();
			assert_eq!(server.read_file("/base/retry.bin").await, b"retried");
		}
	}
}
