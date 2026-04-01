use super::{DataWriterTrait, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use reqwest::Url;
use ssh2::{OpenFlags, OpenType, Session};
use std::{
	io::{Seek, SeekFrom, Write},
	path::{Path, PathBuf},
	thread,
	time::Duration,
};

const MAX_RETRIES: u32 = 2;

/// A struct that provides writing capabilities to a remote file via SFTP.
pub struct DataWriterSftp {
	file: ssh2::File,
	position: u64,
	url: Url,
	identity_file: Option<PathBuf>,
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
			_session: session,
		})
	}

	/// Returns the remote path extracted from an SFTP URL (for extension detection).
	#[must_use]
	pub fn path_from_url(url: &Url) -> PathBuf {
		sftp_utils::remote_path(url)
	}

	/// Reconnect the SFTP session and reopen the file for writing at `self.position`.
	fn reconnect(&mut self) -> Result<()> {
		let name = sftp_utils::display_name(&self.url);
		let path = sftp_utils::remote_path(&self.url);
		log::info!("reconnecting SFTP writer to '{name}'");

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
}

impl DataWriterTrait for DataWriterSftp {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.position;
		let name = sftp_utils::display_name(&self.url);

		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let backoff = Duration::from_secs(1 << (attempt - 1));
				log::warn!(
					"SFTP write retry attempt {attempt}/{MAX_RETRIES} for '{name}' at position {pos}, waiting {backoff:?}"
				);
				thread::sleep(backoff);

				if let Err(e) = self.reconnect() {
					log::warn!("SFTP reconnect failed (attempt {attempt}/{MAX_RETRIES}): {e}");
					if attempt >= MAX_RETRIES {
						return Err(e)
							.with_context(|| format!("failed to reconnect to '{name}' for append at position {pos}"));
					}
					continue;
				}
			}

			match self.file.write_all(blob.as_slice()) {
				Ok(()) => {
					let len = blob.len();
					self.position += len;
					return Ok(ByteRange::new(pos, len));
				}
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!(
						"SFTP write error at position {pos} in '{name}' (attempt {}/{}): {e}",
						attempt + 1,
						MAX_RETRIES + 1
					);
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!(
							"failed to write {} bytes at position {pos} to '{name}' after {} attempts",
							blob.len(),
							MAX_RETRIES + 1
						)
					});
				}
			}
		}

		unreachable!()
	}

	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		let name = sftp_utils::display_name(&self.url);
		let saved = self.position;
		self
			.file
			.seek(SeekFrom::Start(0))
			.with_context(|| format!("failed to seek to start of '{name}'"))?;
		self
			.file
			.write_all(blob.as_slice())
			.with_context(|| format!("failed to write {} bytes at start of '{name}'", blob.len()))?;
		self
			.file
			.seek(SeekFrom::Start(saved))
			.with_context(|| format!("failed to seek back to position {saved} in '{name}'"))?;
		Ok(())
	}

	fn get_position(&mut self) -> Result<u64> {
		Ok(self.position)
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		let name = sftp_utils::display_name(&self.url);
		self
			.file
			.seek(SeekFrom::Start(position))
			.with_context(|| format!("failed to seek to position {position} in '{name}'"))?;
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
