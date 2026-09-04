//! High-level SFTP wrappers that hide the SSH client types from downstream crates.
//!
//! These types allow `versatiles_container` to use SFTP functionality without
//! depending on `russh` directly.
//!

use std::{
	path::{Path, PathBuf},
	pin::Pin,
	task::{Context as TaskContext, Poll},
};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::sftp_utils::{self, Sftp, SshHandle};

/// An [`AsyncWrite`] stream to a remote file via SFTP.
///
/// Keeps the SSH session alive for the lifetime of the writer; russh sends its
/// own keepalives on that session, so the connection survives idle gaps between
/// writes without a thread of ours.
///
/// Writes delegate straight to the underlying `russh_sftp` file, which is
/// already `AsyncWrite` — the stream adds session ownership, nothing else.
pub struct SftpWriteStream {
	file: russh_sftp::client::fs::File,
	/// Dropping the handle closes the session, so the stream owns it even though
	/// it never calls it directly.
	_session: SshHandle,
}

// Held across await points and moved between worker threads, so it has to be
// thread-safe.
const _: fn() = || {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<SftpWriteStream>();
};

impl SftpWriteStream {
	/// Open a remote file for writing from an SFTP URL.
	///
	/// # Errors
	/// Returns an error if the session cannot be opened or the remote file
	/// cannot be created.
	pub async fn from_url(url: &Url, identity_file: Option<&Path>) -> Result<Self> {
		let session = sftp_utils::open_session(url, identity_file).await?;
		let remote_path = sftp_utils::remote_path(url);
		let sftp = sftp_utils::open_sftp(&session).await?;
		let file = sftp
			.create(remote_path.clone())
			.await
			.with_context(|| format!("failed to create remote file {remote_path:?}"))?;

		Ok(Self {
			file,
			_session: session,
		})
	}
}

impl AsyncWrite for SftpWriteStream {
	fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
		Pin::new(&mut self.file).poll_write(cx, buf)
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
		Pin::new(&mut self.file).poll_flush(cx)
	}

	// `File` buffers, so an unflushed tail would be lost. There is no `Drop` that
	// could reach the network — a destructor cannot await — so callers must
	// `shutdown()` the stream, which is what the tar sink's `finish` does.
	fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
		Pin::new(&mut self.file).poll_shutdown(cx)
	}
}

/// A remote filesystem via SFTP for directory-style tile operations.
///
/// Provides `mkdir_p` and `write_file` without exposing SSH client types.
pub struct SftpFileSystem {
	sftp: Sftp,
	session: SshHandle,
	/// `/`-separated remote path, never a local `PathBuf`.
	base_path: String,
	url: Url,
	identity_file: Option<PathBuf>,
}

const _: fn() = || {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<SftpFileSystem>();
};

impl SftpFileSystem {
	/// Connect to an SFTP server and use the URL path as the base directory.
	///
	/// # Errors
	/// Returns an error if the session cannot be opened or the base directory
	/// can neither be created nor found.
	pub async fn from_url(url: &Url, identity_file: Option<&Path>) -> Result<Self> {
		let session = sftp_utils::open_session(url, identity_file).await?;
		let base_path = sftp_utils::remote_path(url);
		let sftp = sftp_utils::open_sftp(&session).await?;

		// Create base directory (ignore error if it already exists)
		if let Err(e) = sftp.create_dir(base_path.clone()).await {
			match sftp.metadata(base_path.clone()).await {
				Ok(metadata) if metadata.is_dir() => {}
				_ => return Err(e).with_context(|| format!("failed to create base directory {base_path:?}")),
			}
		}

		Ok(Self {
			sftp,
			session,
			base_path,
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
		})
	}

	/// Reconnect the SFTP session (e.g. after a network error).
	async fn reconnect(&mut self) -> Result<()> {
		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref()).await?;
		self.sftp = sftp_utils::open_sftp(&session).await?;
		// Replacing the handle drops the old session, closing its transport.
		self.session = session;
		Ok(())
	}

	/// Write a file at `rel_path` relative to the base directory.
	///
	/// Creates parent directories as needed. Retries on error since
	/// `write_file` is idempotent (creates/overwrites a complete file).
	///
	/// # Errors
	/// Returns an error if the file cannot be written after the configured
	/// number of retries.
	pub async fn write_file(&mut self, rel_path: &str, data: &[u8]) -> Result<()> {
		let full_path = sftp_utils::remote_join(&self.base_path, rel_path);

		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;

		for attempt in 0..=max_retries {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!("SFTP write file {full_path:?}: retrying ({attempt_label}, waiting {backoff:?})");
				tokio::time::sleep(backoff).await;
				if let Err(e) = self.reconnect().await {
					log::warn!("SFTP write file {full_path:?}: reconnect failed: {e} ({attempt_label})");
					if attempt >= max_retries {
						return Err(e).with_context(|| {
							format!("could not write file {full_path:?} — reconnect failed after {total_attempts} attempts")
						});
					}
					continue;
				}
			}

			match self.try_write_file(&full_path, data).await {
				Ok(()) => return Ok(()),
				Err(e) if attempt < max_retries => {
					log::warn!("SFTP write file {full_path:?}: {e} ({attempt_label}), will retry");
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!("could not write file {full_path:?} — gave up after {total_attempts} attempts")
					});
				}
			}
		}

		bail!("SFTP write_file retry loop exited without returning — MAX_RETRIES invariant violated")
	}

	async fn try_write_file(&self, full_path: &str, data: &[u8]) -> Result<()> {
		if let Some(parent) = sftp_utils::remote_parent(full_path) {
			self.mkdir_p(parent).await?;
		}

		let mut file = self
			.sftp
			.create(full_path.to_owned())
			.await
			.with_context(|| format!("failed to create remote file {full_path:?}"))?;
		file.write_all(data).await?;
		// `File` buffers, so the bytes are not on the wire until this returns.
		file.flush().await?;
		Ok(())
	}

	/// Create `path` and every missing directory above it.
	///
	/// Iterative rather than recursive: an `async fn` that calls itself needs to
	/// be boxed, and walking the components costs nothing here.
	///
	/// Returns an error if a path component cannot be created and does not
	/// already exist as a directory.
	async fn mkdir_p(&self, path: &str) -> Result<()> {
		let mut prefix = String::new();
		for component in path.split('/').filter(|c| !c.is_empty()) {
			prefix.push('/');
			prefix.push_str(component);

			if let Err(e) = self.sftp.create_dir(prefix.clone()).await {
				// Check whether the directory already exists
				match self.sftp.metadata(prefix.clone()).await {
					Ok(metadata) if metadata.is_dir() => {}
					_ => bail!("failed to create directory {prefix:?}: {e}"),
				}
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test(flavor = "current_thread")]
	async fn test_sftp_write_stream_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		assert!(SftpWriteStream::from_url(&url, None).await.is_err());
	}

	#[tokio::test(flavor = "current_thread")]
	async fn test_sftp_write_stream_with_identity_file() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		assert!(
			SftpWriteStream::from_url(&url, Some(Path::new("/nonexistent/key")))
				.await
				.is_err()
		);
	}

	#[tokio::test(flavor = "current_thread")]
	async fn test_sftp_filesystem_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/").unwrap();
		assert!(SftpFileSystem::from_url(&url, None).await.is_err());
	}

	#[cfg(feature = "sftp")]
	mod sftp_server_tests {
		use super::*;
		use crate::io::test_sftp_server::TestSftpServer;

		// --- SftpWriteStream ---

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_stream_write_and_flush() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			let mut stream = SftpWriteStream::from_url(&url, None).await.unwrap();
			stream.write_all(b"hello world").await.unwrap();
			stream.shutdown().await.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"hello world");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_stream_multiple_writes() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			let mut stream = SftpWriteStream::from_url(&url, None).await.unwrap();
			stream.write_all(b"foo").await.unwrap();
			stream.write_all(b"bar").await.unwrap();
			stream.write_all(b"baz").await.unwrap();
			stream.shutdown().await.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"foobarbaz");
		}

		/// `shutdown` is what lands the buffered tail — a `Drop` impl cannot await,
		/// so unlike the old synchronous stream there is no flush-on-drop.
		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_stream_shutdown_flushes() {
			let server = TestSftpServer::start().await;
			let url = server.url("/out.bin");
			let mut stream = SftpWriteStream::from_url(&url, None).await.unwrap();
			stream.write_all(b"unflushed").await.unwrap();
			stream.shutdown().await.unwrap();
			assert_eq!(server.read_file("/out.bin").await, b"unflushed");
		}

		// --- SftpFileSystem ---

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_file_creates_file() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			let mut fs = SftpFileSystem::from_url(&url, None).await.unwrap();
			fs.write_file("a.bin", b"test data").await.unwrap();
			assert_eq!(server.read_file("/base/a.bin").await, b"test data");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_file_creates_parent_dirs() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			let mut fs = SftpFileSystem::from_url(&url, None).await.unwrap();
			fs.write_file("a/b/c.bin", b"nested").await.unwrap();
			assert_eq!(server.read_file("/base/a/b/c.bin").await, b"nested");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_file_overwrites_existing() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			let mut fs = SftpFileSystem::from_url(&url, None).await.unwrap();
			fs.write_file("a.bin", b"first").await.unwrap();
			fs.write_file("a.bin", b"second").await.unwrap();
			assert_eq!(server.read_file("/base/a.bin").await, b"second");
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn write_file_retry_after_disconnect() {
			let server = TestSftpServer::start().await;
			let url = server.url("/base");
			let mut fs = SftpFileSystem::from_url(&url, None).await.unwrap();
			server.schedule_disconnect();
			fs.write_file("retry.bin", b"retried").await.unwrap();
			assert_eq!(server.read_file("/base/retry.bin").await, b"retried");
		}
	}
}
