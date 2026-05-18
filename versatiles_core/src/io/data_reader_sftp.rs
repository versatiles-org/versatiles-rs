use super::{DataReaderTrait, network_reader::NetworkReader, sftp_pool, sftp_utils};
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::Url;
use std::{
	path::Path,
	sync::{Arc, atomic::AtomicU64},
	thread,
	time::Duration,
};

const MAX_RETRIES: u32 = 2;

/// Exponential backoff unit for retry waits (seconds in prod, ms in tests).
#[cfg(not(test))]
const BACKOFF: fn(u32) -> Duration = |exp| Duration::from_secs(1 << exp);
#[cfg(test)]
const BACKOFF: fn(u32) -> Duration = |exp| Duration::from_millis(1 << exp);

/// A struct that provides reading capabilities from a remote file via SFTP.
///
/// The SSH connection is not owned by the reader: it is borrowed from a
/// per-server connection pool, so many readers of the same server share a
/// small, fixed number of connections instead of opening one each.
pub struct DataReaderSftp {
	connection: Arc<sftp_pool::Connection>,
	file_id: u64,
	size: u64,
	name: String,
	max_request_bytes: AtomicU64,
}

impl std::fmt::Debug for DataReaderSftp {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DataReaderSftp").field("name", &self.name).finish()
	}
}

impl DataReaderSftp {
	/// Opens a remote file for reading via SFTP with an optional identity file.
	///
	/// The underlying SSH connection is taken from the shared per-server
	/// connection pool; opening many files from the same server reuses a
	/// bounded set of connections rather than opening one per file.
	pub fn open(url: &Url, identity_file: Option<&Path>) -> Result<DataReaderSftp> {
		let name = sftp_utils::display_name(url);
		let connection = sftp_pool::acquire(url, identity_file)?;
		let path = sftp_utils::remote_path(url);
		let (file_id, size) = connection
			.register(&path)
			.with_context(|| format!("failed to open '{name}'"))?;

		Ok(DataReaderSftp {
			connection,
			file_id,
			size,
			name,
			max_request_bytes: AtomicU64::new(u64::MAX),
		})
	}
}

impl Drop for DataReaderSftp {
	fn drop(&mut self) {
		// Release the file handle back to the pooled connection so it is not
		// re-opened on future reconnects. The connection itself stays pooled.
		self.connection.unregister(self.file_id);
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
		// Generation observed for the most recent read attempt. Passed to
		// `reconnect` so that sibling readers sharing this pooled connection
		// do not each reconnect the same dropped session.
		let mut generation = self.connection.generation()?;

		for attempt in 0..=MAX_RETRIES {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = BACKOFF(attempt - 1);
				log::warn!("SFTP read {range} from '{name}': retrying ({attempt_label}, waiting {backoff:?})");
				thread::sleep(backoff);

				if let Err(e) = self.connection.reconnect(generation) {
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

			generation = self.connection.generation()?;
			match self.connection.read_range(self.file_id, range) {
				Ok(blob) => return Ok(blob),
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

		bail!("SFTP read retry loop exited without returning — MAX_RETRIES invariant violated")
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

	#[cfg(all(feature = "ssh2", unix))]
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
