use super::{DataReaderTrait, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Url;
use ssh2::Session;
use std::{
	io::{Read, Seek, SeekFrom},
	path::{Path, PathBuf},
	sync::Mutex,
	thread,
	time::Duration,
};

const MAX_RETRIES: u32 = 3;

struct SftpConnection {
	file: ssh2::File,
	// Keep session alive for the lifetime of the connection
	_session: Session,
}

/// A struct that provides reading capabilities from a remote file via SFTP.
pub struct DataReaderSftp {
	connection: Mutex<SftpConnection>,
	size: u64,
	name: String,
	url: Url,
	identity_file: Option<PathBuf>,
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
			connection: Mutex::new(SftpConnection {
				file,
				_session: session,
			}),
			size,
			name,
			url: url.clone(),
			identity_file: identity_file.map(Path::to_path_buf),
		})
	}

	fn reconnect(&self) -> Result<SftpConnection> {
		log::info!("reconnecting SFTP session to '{}'", self.name);
		let session = sftp_utils::open_session(&self.url, self.identity_file.as_deref())?;
		let path = sftp_utils::remote_path(&self.url);
		let sftp = session.sftp()?;
		let file = sftp
			.open(&path)
			.with_context(|| format!("failed to reopen remote file {path:?}"))?;
		Ok(SftpConnection {
			file,
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
		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let backoff = Duration::from_secs(1 << (attempt - 1));
				log::warn!(
					"SFTP retry attempt {attempt}/{MAX_RETRIES} reading range {range} from '{}', waiting {backoff:?}",
					self.name
				);
				thread::sleep(backoff);

				match self.reconnect() {
					Ok(new_conn) => {
						let mut conn = self
							.connection
							.lock()
							.map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
						*conn = new_conn;
					}
					Err(e) => {
						log::warn!("SFTP reconnect failed (attempt {attempt}/{MAX_RETRIES}): {e}");
						if attempt >= MAX_RETRIES {
							return Err(e).with_context(|| {
								format!("failed to reconnect to '{}' after {MAX_RETRIES} attempts", self.name)
							});
						}
						continue;
					}
				}
			}

			let len = usize::try_from(range.length)?;
			let result = {
				let mut conn = self
					.connection
					.lock()
					.map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
				conn.file.seek(SeekFrom::Start(range.offset)).and_then(|_| {
					let mut buffer = vec![0u8; len];
					conn.file.read_exact(&mut buffer).map(|_| buffer)
				})
			};

			match result {
				Ok(buffer) => return Ok(Blob::from(buffer)),
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!(
						"SFTP read error at offset {} len {} from '{}' (attempt {}/{}): {e}",
						range.offset,
						range.length,
						self.name,
						attempt + 1,
						MAX_RETRIES + 1
					);
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!(
							"failed to read {} bytes at offset {} from '{}' after {} attempts",
							range.length,
							range.offset,
							self.name,
							MAX_RETRIES + 1
						)
					});
				}
			}
		}

		unreachable!()
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
