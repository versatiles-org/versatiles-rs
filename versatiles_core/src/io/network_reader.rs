use super::DataReaderTrait;
use crate::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};

/// Trait for network-based data readers (HTTP, SFTP, cloud storage, etc.)
/// that need retry logic and adaptive range splitting.
///
/// Implementors provide `try_read_range` (single attempt with retries) and
/// `max_request_bytes` (atomic size limit). The default `network_read_range`
/// handles proactive splitting and learning from failures.
#[async_trait]
pub(crate) trait NetworkReader: DataReaderTrait {
	/// Attempt to read the given range, with internal retry/backoff logic.
	async fn try_read_range(&self, range: &ByteRange) -> Result<Blob>;

	/// Returns the adaptive maximum request size.
	fn max_request_bytes(&self) -> &AtomicU64;

	/// Reads a range with adaptive splitting.
	///
	/// Proactively splits ranges that exceed a learned size limit.
	/// On failure, splits the range in half and reads each half separately,
	/// recording the failure so future large ranges split proactively.
	async fn network_read_range(&self, range: &ByteRange) -> Result<Blob> {
		// Proactive split: skip try_read_range entirely for ranges we know are too large
		if range.length > self.max_request_bytes().load(Ordering::Relaxed) && range.length > 1 {
			log::info!(
				"proactively splitting range {range} ({} bytes) based on previous failures",
				range.length
			);
			return self.split_and_read(range).await;
		}

		match self.try_read_range(range).await {
			Ok(blob) => Ok(blob),
			Err(e) if range.length <= 1 => Err(e),
			Err(e) => {
				// Learn from failure: future ranges this large should split proactively
				self.max_request_bytes().fetch_min(range.length / 2, Ordering::Relaxed);
				log::warn!(
					"splitting failed range {range} ({} bytes) into two halves: {e}",
					range.length
				);
				self.split_and_read(range).await
			}
		}
	}

	/// Splits a range in half and reads each half via `read_range` (which
	/// may recurse back into `network_read_range`).
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
