use super::{DataReaderTrait, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Url;
use ssh2::Session;
use std::{
	io::{Read, Seek, SeekFrom},
	path::{Path, PathBuf},
	sync::{
		Mutex,
		atomic::{AtomicU64, Ordering},
	},
	thread,
	time::Duration,
};

const MAX_RETRIES: u32 = 2;

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
	max_request_bytes: AtomicU64,
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
			max_request_bytes: AtomicU64::new(u64::MAX),
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

impl DataReaderSftp {
	/// Single-range read with retry/backoff/reconnect. Called by the trait
	/// `read_range`, which adds divide-and-conquer splitting on top.
	fn try_read_range(&self, range: &ByteRange) -> Result<Blob> {
		let total_attempts = MAX_RETRIES + 1;
		let name = &self.name;
		let len = range.length;

		for attempt in 0..=MAX_RETRIES {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = Duration::from_secs(1 << (attempt - 1));
				log::warn!("SFTP read {range} from '{name}': retrying ({attempt_label}, waiting {backoff:?})");
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
						log::warn!("SFTP read {range} from '{name}': reconnect failed ({attempt_label}): {e}");
						if attempt >= MAX_RETRIES {
							return Err(e).with_context(|| {
								format!(
									"could not read {range} ({len} bytes) from '{name}': reconnect failed — gave up after {total_attempts} attempts"
								)
							});
						}
						continue;
					}
				}
			}

			let buf_len = usize::try_from(range.length)?;
			let result = {
				let mut conn = self
					.connection
					.lock()
					.map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
				conn.file.seek(SeekFrom::Start(range.offset)).and_then(|_| {
					let mut buffer = vec![0u8; buf_len];
					conn.file.read_exact(&mut buffer).map(|_| buffer)
				})
			};

			match result {
				Ok(buffer) => return Ok(Blob::from(buffer)),
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!("SFTP read {range} from '{name}': {e} ({attempt_label}), will retry");
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!(
							"could not read {range} ({len} bytes) from '{name}' — gave up after {total_attempts} attempts"
						)
					});
				}
			}
		}

		unreachable!()
	}

	async fn split_and_read(&self, range: &ByteRange) -> Result<Blob> {
		let mid = range.offset + range.length / 2;
		let left = ByteRange::new(range.offset, mid - range.offset);
		let right = ByteRange::new(mid, range.offset + range.length - mid);
		let blob_left = self.read_range(&left).await?;
		let blob_right = self.read_range(&right).await?;
		let mut data = blob_left.into_vec();
		data.extend_from_slice(blob_right.as_slice());
		Ok(Blob::from(data))
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderSftp {
	/// Reads a specific range of bytes from the SFTP endpoint.
	///
	/// Proactively splits ranges that exceed a learned size limit.
	/// On failure, splits the range in half and reads each half separately.
	/// This recurses until the pieces are small enough to succeed on a flaky
	/// connection.
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		// Proactive split: skip try_read_range entirely for ranges we know are too large
		if range.length > self.max_request_bytes.load(Ordering::Relaxed) && range.length > 1 {
			log::info!(
				"proactively splitting range {range} ({} bytes) based on previous failures",
				range.length
			);
			return self.split_and_read(range).await;
		}

		match self.try_read_range(range) {
			Ok(blob) => Ok(blob),
			Err(e) if range.length <= 1 => Err(e),
			Err(e) => {
				// Learn from failure: future ranges this large should split proactively
				self.max_request_bytes.fetch_min(range.length / 2, Ordering::Relaxed);
				log::warn!(
					"splitting failed range {range} ({} bytes) into two halves: {e}",
					range.length
				);
				self.split_and_read(range).await
			}
		}
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
