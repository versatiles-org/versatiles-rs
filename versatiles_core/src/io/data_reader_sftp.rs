use super::{DataReaderTrait, network_reader::NetworkReader, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Url;
use ssh2::Session;
use std::{
	io::{Read, Seek, SeekFrom},
	path::{Path, PathBuf},
	sync::{Mutex, atomic::AtomicU64},
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
	/// Single-range read with retry/backoff/reconnect.
	fn try_read_range_impl(&self, range: &ByteRange) -> Result<Blob> {
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
}

#[async_trait]
impl NetworkReader for DataReaderSftp {
	async fn try_read_range(&self, range: &ByteRange) -> Result<Blob> {
		self.try_read_range_impl(range)
	}

	fn max_request_bytes(&self) -> &AtomicU64 {
		&self.max_request_bytes
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderSftp {
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		self.network_read_range(range).await
	}

	async fn read_all(&self) -> Result<Blob> {
		self.read_range(&ByteRange::new(0, self.size)).await
	}

	fn name(&self) -> &str {
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

	#[cfg(feature = "ssh2")]
	mod sftp_server_tests {
		use super::*;
		use crate::{ByteRange, io::test_sftp_server::TestSftpServer};

		#[tokio::test(flavor = "multi_thread")]
		async fn read_all_returns_correct_bytes() {
			let server = TestSftpServer::start().await;
			let data: Vec<u8> = (0u8..100).collect();
			server.write_file("/test.bin", &data).await;
			let url = server.url("/test.bin");
			let reader = tokio::task::spawn_blocking(move || DataReaderSftp::open(&url, None))
				.await
				.unwrap()
				.unwrap();
			let result = reader.read_all().await.unwrap();
			assert_eq!(result.as_slice(), data.as_slice());
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn read_range_returns_slice() {
			let server = TestSftpServer::start().await;
			let data: Vec<u8> = (0u8..100).collect();
			server.write_file("/test.bin", &data).await;
			let url = server.url("/test.bin");
			let reader = tokio::task::spawn_blocking(move || DataReaderSftp::open(&url, None))
				.await
				.unwrap()
				.unwrap();
			let result = reader.read_range(&ByteRange::new(20, 30)).await.unwrap();
			assert_eq!(result.as_slice(), &data[20..50]);
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn read_range_at_eof() {
			let server = TestSftpServer::start().await;
			server.write_file("/test.bin", b"hello").await;
			let url = server.url("/test.bin");
			let reader = tokio::task::spawn_blocking(move || DataReaderSftp::open(&url, None))
				.await
				.unwrap()
				.unwrap();
			let result = reader.read_range(&ByteRange::new(5, 0)).await.unwrap();
			assert!(result.is_empty());
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn read_retry_after_disconnect() {
			let server = TestSftpServer::start().await;
			let data: Vec<u8> = (0u8..50).collect();
			server.write_file("/test.bin", &data).await;
			let url = server.url("/test.bin");
			let reader = tokio::task::spawn_blocking(move || DataReaderSftp::open(&url, None))
				.await
				.unwrap()
				.unwrap();
			server.schedule_disconnect();
			let result = reader.read_range(&ByteRange::new(0, 50)).await.unwrap();
			assert_eq!(result.as_slice(), data.as_slice());
		}

		#[tokio::test(flavor = "multi_thread")]
		async fn open_nonexistent_file_errors() {
			let server = TestSftpServer::start().await;
			let url = server.url("/nonexistent.bin");
			let result = tokio::task::spawn_blocking(move || DataReaderSftp::open(&url, None))
				.await
				.unwrap();
			assert!(result.is_err());
		}
	}
}
