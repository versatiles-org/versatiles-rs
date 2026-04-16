use super::{DataWriterTrait, network_writer::NetworkWriter, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use reqwest::Url;
use ssh2::{OpenFlags, OpenType, Session};
use std::{
	io::{Seek, SeekFrom, Write},
	path::{Path, PathBuf},
};

/// A struct that provides writing capabilities to a remote file via SFTP.
pub struct DataWriterSftp {
	file: ssh2::File,
	position: u64,
	url: Url,
	identity_file: Option<PathBuf>,
	name: String,
	// Keep session alive for the lifetime of the writer
	_session: Session,
}

impl DataWriterSftp {
	/// Opens a remote file for writing via SFTP.
	///
	/// # Arguments
	/// * `url` - A parsed SFTP URL
	///
	/// # Authentication priority
	/// 1. Credentials in URL (password auth)
	/// 2. SSH agent
	/// 3. Default key files (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
	pub fn from_url(url: &Url, identity_file: Option<&Path>) -> Result<Self> {
		let session = sftp_utils::open_session(url, identity_file)?;
		let path = sftp_utils::remote_path(url);

		let sftp = session.sftp()?;
		let file = sftp
			.create(&path)
			.with_context(|| format!("failed to create remote file {path:?}"))?;

		Ok(DataWriterSftp {
			file,
			position: 0,
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
			name: sftp_utils::display_name(url),
			_session: session,
		})
	}

	/// Returns the remote path extracted from an SFTP URL (for extension detection).
	#[must_use]
	pub fn path_from_url(url: &Url) -> PathBuf {
		sftp_utils::remote_path(url)
	}
}

impl NetworkWriter for DataWriterSftp {
	fn try_append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.position;
		self.file.write_all(blob.as_slice())?;
		self.position += blob.len();
		Ok(ByteRange::new(pos, blob.len()))
	}

	fn try_write_at(&mut self, offset: u64, blob: &Blob, restore_pos: u64) -> Result<()> {
		self
			.file
			.seek(SeekFrom::Start(offset))
			.with_context(|| format!("failed to seek to offset {offset} in '{}'", self.name))?;
		self.file.write_all(blob.as_slice()).with_context(|| {
			format!(
				"failed to write {} bytes at offset {offset} in '{}'",
				blob.len(),
				self.name
			)
		})?;
		self
			.file
			.seek(SeekFrom::Start(restore_pos))
			.with_context(|| format!("failed to seek back to position {restore_pos} in '{}'", self.name))?;
		Ok(())
	}

	fn try_seek(&mut self, position: u64) -> Result<()> {
		self
			.file
			.seek(SeekFrom::Start(position))
			.with_context(|| format!("failed to seek to position {position} in '{}'", self.name))?;
		self.position = position;
		Ok(())
	}

	fn reconnect(&mut self) -> Result<()> {
		let path = sftp_utils::remote_path(&self.url);
		log::info!("reconnecting SFTP writer to '{}'", self.name);

		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref())?;
		let sftp = session.sftp()?;
		let mut file = sftp
			.open_mode(&path, OpenFlags::WRITE, 0o644, OpenType::File)
			.with_context(|| format!("failed to reopen remote file {path:?} for writing"))?;
		file
			.seek(SeekFrom::Start(self.position))
			.with_context(|| format!("failed to seek to position {} in {path:?}", self.position))?;

		self.file = file;
		self._session = session;
		Ok(())
	}

	fn writer_name(&self) -> &str {
		&self.name
	}

	fn tracked_position(&self) -> u64 {
		self.position
	}
}

impl DataWriterTrait for DataWriterSftp {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		self.network_append(blob)
	}

	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		self.network_write_start(blob)
	}

	fn position(&mut self) -> Result<u64> {
		Ok(self.position)
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		self.network_set_position(position)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_path_from_url() {
		let url = Url::parse("sftp://host/data/out.versatiles").unwrap();
		assert_eq!(
			DataWriterSftp::path_from_url(&url),
			PathBuf::from("/data/out.versatiles")
		);
	}

	#[test]
	fn test_path_from_url_with_credentials() {
		let url = Url::parse("sftp://user:pass@host:2222/output/tiles.tar").unwrap();
		assert_eq!(DataWriterSftp::path_from_url(&url), PathBuf::from("/output/tiles.tar"));
	}

	#[test]
	fn test_path_from_url_root() {
		let url = Url::parse("sftp://host/file.versatiles").unwrap();
		assert_eq!(DataWriterSftp::path_from_url(&url), PathBuf::from("/file.versatiles"));
	}

	#[test]
	fn test_path_from_url_nested() {
		let url = Url::parse("sftp://host/a/b/c/d.tar").unwrap();
		assert_eq!(DataWriterSftp::path_from_url(&url), PathBuf::from("/a/b/c/d.tar"));
	}

	#[test]
	fn test_from_url_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = DataWriterSftp::from_url(&url, None);
		assert!(result.is_err());
	}
}
