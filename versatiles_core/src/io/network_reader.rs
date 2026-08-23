use std::{
	fmt,
	sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use async_trait::async_trait;

use super::DataReaderTrait;
use crate::{Blob, ByteRange};

/// Attached to an error that a smaller range would run into just the same.
///
/// Splitting exists for peers that reject a request because it is *large* — a
/// proxy with a response cap, a server that times out on a long transfer. It
/// does nothing for a peer that answered perfectly promptly with the wrong
/// thing: a `200` because it ignores `Range` headers, a `404`, a `416` for a
/// range past the end of the file. Halving such a request just asks the same
/// question twice, and recursion turns one mistake into a tree of them —
/// measured at 110 requests for a single bogus range out of a crafted
/// PMTiles header.
#[derive(Debug)]
pub(crate) struct SmallerRangeWontHelp;

impl fmt::Display for SmallerRangeWontHelp {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("retrying with smaller ranges would not help")
	}
}

impl std::error::Error for SmallerRangeWontHelp {}

/// Whether splitting `error` into two half-sized reads is worth attempting.
///
/// `downcast_ref` rather than a walk over `chain()`: anyhow stores a context
/// value in a wrapper type, so the chain yields that wrapper and not the marker
/// itself, while `downcast_ref` looks through the context layers.
fn splitting_could_help(error: &anyhow::Error) -> bool {
	error.downcast_ref::<SmallerRangeWontHelp>().is_none()
}

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
			Err(e) if range.length <= 1 || !splitting_could_help(&e) => Err(e),
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
	use std::{
		fmt,
		sync::{
			Arc,
			atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering},
		},
		time::Duration,
	};

	use super::*;

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

	/// A reader that always fails, counting how often it was asked.
	struct FailingReader {
		calls: AtomicUsize,
		max_request: AtomicU64,
		splittable: bool,
	}

	impl fmt::Debug for FailingReader {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.debug_struct("FailingReader").finish()
		}
	}

	#[async_trait]
	impl DataReaderTrait for FailingReader {
		async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
			self.network_read_range(range).await
		}
		async fn read_all(&self) -> Result<Blob> {
			unreachable!("FailingReader only used for read_range")
		}
		fn name(&self) -> &str {
			"failing"
		}
	}

	#[async_trait]
	impl NetworkReader for FailingReader {
		async fn try_read_range(&self, _range: &ByteRange) -> Result<Blob> {
			self.calls.fetch_add(1, AtomicOrdering::SeqCst);
			let error = anyhow::anyhow!("nope");
			Err(if self.splittable {
				error
			} else {
				error.context(SmallerRangeWontHelp)
			})
		}
		fn max_request_bytes(&self) -> &AtomicU64 {
			&self.max_request
		}
	}

	/// An error that smaller ranges cannot fix is reported after one attempt.
	/// Before the marker existed, a crafted PMTiles header pointing at a range
	/// the server answers with `200` cost 110 requests before failing.
	#[tokio::test]
	async fn an_unfixable_error_is_not_split() {
		let reader = FailingReader {
			calls: AtomicUsize::new(0),
			max_request: AtomicU64::new(u64::MAX),
			splittable: false,
		};

		let result = reader.network_read_range(&ByteRange::new(0, 1 << 20)).await;

		assert!(result.is_err());
		assert_eq!(
			reader.calls.load(AtomicOrdering::SeqCst),
			1,
			"an unfixable error must not be retried in halves"
		);
	}

	/// A plain failure still splits — that is what the mechanism is for.
	#[tokio::test]
	async fn a_plain_error_still_splits() {
		let reader = FailingReader {
			calls: AtomicUsize::new(0),
			max_request: AtomicU64::new(u64::MAX),
			splittable: true,
		};

		let result = reader.network_read_range(&ByteRange::new(0, 64)).await;

		assert!(result.is_err());
		assert!(
			reader.calls.load(AtomicOrdering::SeqCst) > 1,
			"a size-related failure should still be retried in halves"
		);
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
