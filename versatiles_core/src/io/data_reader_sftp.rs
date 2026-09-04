use std::{
	path::Path,
	sync::{Arc, atomic::AtomicU64},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::Url;

use super::{DataReaderTrait, network_reader::NetworkReader, sftp_pool, sftp_utils};
use crate::{Blob, ByteRange};

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
	pub async fn open(url: &Url, identity_file: Option<&Path>) -> Result<DataReaderSftp> {
		let name = sftp_utils::display_name(url);
		let connection = sftp_pool::acquire(url, identity_file).await?;
		let path = sftp_utils::remote_path(url);
		let (file_id, size) = connection
			.register(&path)
			.await
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
		//
		// `unregister` needs the connection lock and `Drop` cannot await, so the
		// release is handed to the SFTP runtime and allowed to finish on its own.
		// Losing it would only mean the handle is reopened on the next reconnect,
		// so there is nothing to wait for.
		let connection = Arc::clone(&self.connection);
		let file_id = self.file_id;
		super::sftp_blocking::spawn(async move { connection.unregister(file_id).await });
	}
}

impl DataReaderSftp {
	/// Single-range read with retry/backoff/reconnect.
	async fn try_read_range_impl(&self, range: &ByteRange) -> Result<Blob> {
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;
		let name = &self.name;
		let len = range.length;
		// Generation observed for the most recent read attempt. Passed to
		// `reconnect` so that sibling readers sharing this pooled connection
		// do not each reconnect the same dropped session.
		let mut generation = self.connection.generation().await;

		for attempt in 0..=max_retries {
			let attempt_label = format!("attempt {}/{total_attempts}", attempt + 1);

			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!("SFTP read {range} from '{name}': retrying ({attempt_label}, waiting {backoff:?})");
				tokio::time::sleep(backoff).await;

				if let Err(e) = self.connection.reconnect(generation).await {
					log::warn!("SFTP read {range} from '{name}': reconnect failed ({attempt_label}): {e}");
					if attempt >= max_retries {
						return Err(e).with_context(|| {
							format!(
								"could not read {range} ({len} bytes) from '{name}': reconnect failed — gave up after {total_attempts} attempts"
							)
						});
					}
					continue;
				}
			}

			generation = self.connection.generation().await;
			match self.connection.read_range(self.file_id, range).await {
				Ok(blob) => return Ok(blob),
				Err(e) if attempt < max_retries => {
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
		self.try_read_range_impl(range).await
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

	#[tokio::test(flavor = "current_thread")]
	async fn test_open_unreachable_host() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		assert!(DataReaderSftp::open(&url, None).await.is_err());
	}

	#[tokio::test(flavor = "current_thread")]
	async fn test_open_with_identity_file_unreachable() {
		let url = Url::parse("sftp://192.0.2.1:22222/path/file.versatiles").unwrap();
		assert!(
			DataReaderSftp::open(&url, Some(Path::new("/nonexistent/key")))
				.await
				.is_err()
		);
	}

	#[cfg(feature = "sftp")]
	mod sftp_server_tests {
		use super::*;
		use crate::{ByteRange, io::test_sftp_server::TestSftpServer};

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn read_all_returns_correct_bytes() {
			let server = TestSftpServer::start().await;
			let data: Vec<u8> = (0u8..100).collect();
			server.write_file("/test.bin", &data).await;
			let url = server.url("/test.bin");
			let reader = DataReaderSftp::open(&url, None).await.unwrap();
			let result = reader.read_all().await.unwrap();
			assert_eq!(result.as_slice(), data.as_slice());
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn read_range_returns_slice() {
			let server = TestSftpServer::start().await;
			let data: Vec<u8> = (0u8..100).collect();
			server.write_file("/test.bin", &data).await;
			let url = server.url("/test.bin");
			let reader = DataReaderSftp::open(&url, None).await.unwrap();
			let result = reader.read_range(&ByteRange::new(20, 30)).await.unwrap();
			assert_eq!(result.as_slice(), &data[20..50]);
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn read_range_at_eof() {
			let server = TestSftpServer::start().await;
			server.write_file("/test.bin", b"hello").await;
			let url = server.url("/test.bin");
			let reader = DataReaderSftp::open(&url, None).await.unwrap();
			let result = reader.read_range(&ByteRange::new(5, 0)).await.unwrap();
			assert!(result.is_empty());
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn read_retry_after_disconnect() {
			let server = TestSftpServer::start().await;
			let data: Vec<u8> = (0u8..50).collect();
			server.write_file("/test.bin", &data).await;
			let url = server.url("/test.bin");
			let reader = DataReaderSftp::open(&url, None).await.unwrap();
			server.schedule_disconnect();
			let result = reader.read_range(&ByteRange::new(0, 50)).await.unwrap();
			assert_eq!(result.as_slice(), data.as_slice());
		}

		#[tokio::test(flavor = "current_thread")]
		#[serial_test::serial]
		async fn open_nonexistent_file_errors() {
			let server = TestSftpServer::start().await;
			let url = server.url("/nonexistent.bin");
			assert!(DataReaderSftp::open(&url, None).await.is_err());
		}
	}
}
