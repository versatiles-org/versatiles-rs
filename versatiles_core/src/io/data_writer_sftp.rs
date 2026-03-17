use super::{DataWriterTrait, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use reqwest::Url;
use ssh2::Session;
use std::{
	io::{Seek, SeekFrom, Write},
	path::PathBuf,
};

/// A struct that provides writing capabilities to a remote file via SFTP.
pub struct DataWriterSftp {
	file: ssh2::File,
	position: u64,
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
	pub fn from_url(url: &Url) -> Result<Self> {
		let session = sftp_utils::open_session(url)?;
		let path = sftp_utils::remote_path(url);

		let sftp = session.sftp()?;
		let file = sftp
			.create(&path)
			.with_context(|| format!("failed to create remote file {:?}", path))?;

		Ok(DataWriterSftp {
			file,
			position: 0,
			_session: session,
		})
	}

	/// Returns the remote path extracted from an SFTP URL (for extension detection).
	#[must_use]
	pub fn path_from_url(url: &Url) -> PathBuf {
		sftp_utils::remote_path(url)
	}
}

impl DataWriterTrait for DataWriterSftp {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.position;
		self.file.write_all(blob.as_slice())?;
		let len = blob.len();
		self.position += len;
		Ok(ByteRange::new(pos, len))
	}

	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		let saved = self.position;
		self.file.seek(SeekFrom::Start(0))?;
		self.file.write_all(blob.as_slice())?;
		self.file.seek(SeekFrom::Start(saved))?;
		Ok(())
	}

	fn get_position(&mut self) -> Result<u64> {
		Ok(self.position)
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		self.file.seek(SeekFrom::Start(position))?;
		self.position = position;
		Ok(())
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
}
