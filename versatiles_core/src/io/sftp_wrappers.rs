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
			_session: session,
		})
	}

	/// Write a file at `rel_path` relative to the base directory.
	///
	/// Creates parent directories as needed. Retries on error since
	/// `write_file` is idempotent (creates/overwrites a complete file).
	pub fn write_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
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
}
