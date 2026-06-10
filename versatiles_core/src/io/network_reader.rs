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
		log::trace!("network_read_range {range} ({} bytes)", range.length);
		// Short-circuit zero-length reads. HTTP `Range: bytes=N-(N-1)` is malformed
		// and conforming servers reject it with 400; SFTP would also reject. Empty
		// "ranges" appear in PMTiles when a directory section is absent.
		if range.length == 0 {
			return Ok(Blob::default());
		}

		// Proactive split: skip try_read_range entirely for ranges we know are too large
		if range.length > self.max_request_bytes().load(Ordering::Relaxed) && range.length > 1 {
			log::trace!(
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
				log::debug!(
					"splitting failed range {range} ({} bytes) into two halves: {e}",
					range.length
				);
				self.split_and_read(range).await
			}
		}
	}

	/// Splits a range in half and reads each half via `read_range` (which
	/// may recurse back into `network_read_range`). The two halves are
	/// fetched concurrently so wall-clock stays near max(left, right)
	/// instead of left + right.
	async fn split_and_read(&self, range: &ByteRange) -> Result<Blob> {
		let mid = range.offset + range.length / 2;
		let left = ByteRange::new(range.offset, mid - range.offset);
		let right = ByteRange::new(mid, range.offset + range.length - mid);
		log::trace!("split_and_read {range} -> [{left}] + [{right}]");
		let (blob_left, blob_right) = futures::future::try_join(self.read_range(&left), self.read_range(&right)).await?;
		let mut data = blob_left.into_vec();
		data.extend_from_slice(blob_right.as_slice());
		Ok(Blob::from(data))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		fmt,
		sync::{
			Arc,
			atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering},
		},
		time::Duration,
	};

	#[derive(Default)]
	struct PeakState {
		in_flight: AtomicUsize,
		max_in_flight: AtomicUsize,
		max_request: AtomicU64,
	}

	struct PeakNetReader {
		state: Arc<PeakState>,
		delay: Duration,
	}

	impl fmt::Debug for PeakNetReader {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.debug_struct("PeakNetReader").finish()
		}
	}

	#[async_trait]
	impl DataReaderTrait for PeakNetReader {
		async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
			self.network_read_range(range).await
		}
		async fn read_all(&self) -> Result<Blob> {
			unreachable!("PeakNetReader only used for read_range")
		}
		fn name(&self) -> &str {
			"peak-net"
		}
	}

	#[async_trait]
	impl NetworkReader for PeakNetReader {
		async fn try_read_range(&self, range: &ByteRange) -> Result<Blob> {
			let n = self.state.in_flight.fetch_add(1, AtomicOrdering::SeqCst) + 1;
			self.state.max_in_flight.fetch_max(n, AtomicOrdering::SeqCst);
			tokio::time::sleep(self.delay).await;
			self.state.in_flight.fetch_sub(1, AtomicOrdering::SeqCst);
			Ok(Blob::from(vec![0u8; usize::try_from(range.length).unwrap()]))
		}
		fn max_request_bytes(&self) -> &AtomicU64 {
			&self.state.max_request
		}
	}

	#[tokio::test]
	async fn zero_length_read_short_circuits() {
		let state = Arc::new(PeakState {
			in_flight: AtomicUsize::new(0),
			max_in_flight: AtomicUsize::new(0),
			max_request: AtomicU64::new(u64::MAX),
		});
		let reader = PeakNetReader {
			state: Arc::clone(&state),
			delay: Duration::from_millis(10),
		};
		let blob = reader.network_read_range(&ByteRange::new(4117, 0)).await.unwrap();
		assert_eq!(blob.len(), 0);
		// No call should have reached try_read_range — no malformed HTTP range sent.
		assert_eq!(state.max_in_flight.load(AtomicOrdering::SeqCst), 0);
	}

	#[tokio::test]
	async fn split_and_read_runs_halves_concurrently() {
		let state = Arc::new(PeakState {
			in_flight: AtomicUsize::new(0),
			max_in_flight: AtomicUsize::new(0),
			// Force proactive split: any range > 10 bytes splits before issuing.
			max_request: AtomicU64::new(10),
		});
		let reader = PeakNetReader {
			state: Arc::clone(&state),
			delay: Duration::from_millis(40),
		};

		let blob = reader.network_read_range(&ByteRange::new(0, 100)).await.unwrap();

		assert_eq!(blob.len(), 100);
		let peak = state.max_in_flight.load(AtomicOrdering::SeqCst);
		assert!(peak >= 2, "expected concurrent split halves, saw peak {peak} in flight");
	}
}
