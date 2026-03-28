use super::{DataReaderTrait, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Url;
use ssh2::Session;
use std::{
	io::{Read, Seek, SeekFrom},
	path::Path,
	sync::Mutex,
};

/// A struct that provides reading capabilities from a remote file via SFTP.
pub struct DataReaderSftp {
	file: Mutex<ssh2::File>,
	size: u64,
	name: String,
	// Keep session alive for the lifetime of the reader
	_session: Session,
}

impl std::fmt::Debug for DataReaderSftp {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DataReaderSftp").field("name", &self.name).finish()
	}
}

impl DataReaderSftp {
	/// Opens a remote file for reading via SFTP with an optional identity file.
	pub fn open(url: &Url, identity_file: Option<&Path>) -> Result<DataReaderSftp> {
		let session = sftp_utils::open_session(url, identity_file)?;
		let path = sftp_utils::remote_path(url);
		let name = sftp_utils::display_name(url);

		let sftp = session.sftp()?;
		let stat = sftp
			.stat(&path)
			.with_context(|| format!("failed to stat remote file {path:?}"))?;
		let size = stat.size.unwrap_or(0);

		let file = sftp
			.open(&path)
			.with_context(|| format!("failed to open remote file {path:?}"))?;

		Ok(DataReaderSftp {
			file: Mutex::new(file),
			size,
			name,
			_session: session,
		})
	}
}

impl TryFrom<&Url> for DataReaderSftp {
	type Error = anyhow::Error;

	/// Opens a remote file for reading via SFTP (no explicit identity file).
	fn try_from(url: &Url) -> Result<DataReaderSftp> {
		Self::open(url, None)
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderSftp {
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let mut file = self.file.lock().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
		file.seek(SeekFrom::Start(range.offset))?;
		let mut buffer = vec![0u8; usize::try_from(range.length)?];
		file.read_exact(&mut buffer).with_context(|| {
			format!(
				"failed to read {} bytes at offset {} from '{}'",
				range.length, range.offset, self.name
			)
		})?;
		Ok(Blob::from(buffer))
	}

	async fn read_all(&self) -> Result<Blob> {
		self.read_range(&ByteRange::new(0, self.size)).await
	}

	fn get_name(&self) -> &str {
		&self.name
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_open_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = DataReaderSftp::open(&url, None);
		assert!(result.is_err());
	}

	#[test]
	fn test_try_from_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = DataReaderSftp::try_from(&url);
		assert!(result.is_err());
	}

	#[test]
	fn test_open_with_identity_file_unreachable() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		let result = DataReaderSftp::open(&url, Some(Path::new("/nonexistent/key")));
		assert!(result.is_err());
	}
}
