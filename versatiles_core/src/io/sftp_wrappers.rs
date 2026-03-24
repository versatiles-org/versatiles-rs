//! High-level SFTP wrappers that hide `ssh2` types from downstream crates.
//!
//! These types allow `versatiles_container` to use SFTP functionality without
//! depending on the `ssh2` crate directly.

use super::sftp_utils;
use anyhow::{Context, Result};
use reqwest::Url;
use std::{
	io::Write,
	path::{Path, PathBuf},
};

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

		// Create base directory (ignore error if exists)
		let _ = sftp.mkdir(&base_path, 0o755);

		Ok(Self {
			sftp,
			base_path,
			_session: session,
		})
	}

	/// Write a file at `rel_path` relative to the base directory.
	///
	/// Creates parent directories as needed.
	pub fn write_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
		let full_path = self.base_path.join(rel_path);

		// Create parent directories
		if let Some(parent) = full_path.parent() {
			self.mkdir_p(parent);
		}

		let mut file = self
			.sftp
			.create(&full_path)
			.with_context(|| format!("failed to create remote file {full_path:?}"))?;
		file.write_all(data)?;
		Ok(())
	}

	/// Recursively create directories, ignoring errors for existing dirs.
	fn mkdir_p(&self, path: &Path) {
		if let Some(parent) = path.parent() {
			self.mkdir_p(parent);
		}
		let _ = self.sftp.mkdir(path, 0o755);
	}
}
