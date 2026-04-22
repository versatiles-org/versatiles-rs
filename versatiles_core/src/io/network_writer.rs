use super::DataWriterTrait;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, bail};
use std::{thread, time::Duration};

const MAX_RETRIES: u32 = 2;

/// Exponential backoff unit for retry waits.
///
/// In production this is one second so retries wait 1 s, 2 s, … In tests the
/// unit shrinks to a few milliseconds to keep the retry-path tests fast while
/// still exercising the `thread::sleep` call itself.
#[cfg(not(test))]
const BACKOFF: fn(u32) -> Duration = |exp| Duration::from_secs(1 << exp);
#[cfg(test)]
const BACKOFF: fn(u32) -> Duration = |exp| Duration::from_millis(1 << exp);

/// Trait for network-based data writers (SFTP, cloud storage, etc.)
/// that need retry logic with reconnection.
///
/// Implementors provide single-attempt operations and reconnection logic.
/// The default `network_*` methods handle retry loops with exponential backoff.
pub(crate) trait NetworkWriter: DataWriterTrait {
	/// Single-attempt write at current position.
	fn try_append(&mut self, blob: &Blob) -> Result<ByteRange>;

	/// Single-attempt: seek to `offset`, write `blob`, seek back to `restore_pos`.
	fn try_write_at(&mut self, offset: u64, blob: &Blob, restore_pos: u64) -> Result<()>;

	/// Single-attempt seek to `position`.
	fn try_seek(&mut self, position: u64) -> Result<()>;

	/// Re-establish the connection (new session, reopen file, seek to tracked position).
	fn reconnect(&mut self) -> Result<()>;

	/// Display name for log messages.
	fn writer_name(&self) -> &str;

	/// Current tracked write position (no I/O).
	fn tracked_position(&self) -> u64;

	/// Append with retry and reconnect on failure.
	fn network_append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let name = self.writer_name().to_string();
		let pos = self.tracked_position();
		let blob_len = blob.len();
		let total_attempts = MAX_RETRIES + 1;

		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let backoff = BACKOFF(attempt - 1);
				log::warn!(
					"write to '{name}' at position {pos}: retrying (attempt {}/{total_attempts}, waiting {backoff:?})",
					attempt + 1
				);
				thread::sleep(backoff);

				if let Err(e) = self.reconnect() {
					log::warn!(
						"write to '{name}' at position {pos}: reconnect failed (attempt {}/{total_attempts}): {e}",
						attempt + 1
					);
					if attempt >= MAX_RETRIES {
						return Err(e).with_context(|| {
							format!("could not write {blob_len} bytes at position {pos} to '{name}': reconnect failed — gave up after {total_attempts} attempts")
						});
					}
					continue;
				}
			}

			match self.try_append(blob) {
				Ok(range) => return Ok(range),
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!(
						"write to '{name}' at position {pos}: {e} (attempt {}/{total_attempts}), will retry",
						attempt + 1
					);
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!(
							"could not write {blob_len} bytes at position {pos} to '{name}' — gave up after {total_attempts} attempts"
						)
					});
				}
			}
		}

		bail!("retry loop exited without returning — MAX_RETRIES invariant violated")
	}

	/// Write at start of file with retry and reconnect on failure.
	fn network_write_start(&mut self, blob: &Blob) -> Result<()> {
		let name = self.writer_name().to_string();
		let blob_len = blob.len();
		let total_attempts = MAX_RETRIES + 1;

		for attempt in 0..=MAX_RETRIES {
			let restore_pos = self.tracked_position();

			if attempt > 0 {
				let backoff = BACKOFF(attempt - 1);
				log::warn!(
					"write_start to '{name}': retrying (attempt {}/{total_attempts}, waiting {backoff:?})",
					attempt + 1
				);
				thread::sleep(backoff);

				if let Err(e) = self.reconnect() {
					log::warn!(
						"write_start to '{name}': reconnect failed (attempt {}/{total_attempts}): {e}",
						attempt + 1
					);
					if attempt >= MAX_RETRIES {
						return Err(e).with_context(|| {
							format!("could not write {blob_len} bytes at start of '{name}': reconnect failed — gave up after {total_attempts} attempts")
						});
					}
					continue;
				}
			}

			match self.try_write_at(0, blob, restore_pos) {
				Ok(()) => return Ok(()),
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!(
						"write_start to '{name}': {e} (attempt {}/{total_attempts}), will retry",
						attempt + 1
					);
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!(
							"could not write {blob_len} bytes at start of '{name}' — gave up after {total_attempts} attempts"
						)
					});
				}
			}
		}

		bail!("retry loop exited without returning — MAX_RETRIES invariant violated")
	}

	/// Seek with retry and reconnect on failure.
	fn network_set_position(&mut self, position: u64) -> Result<()> {
		let name = self.writer_name().to_string();
		let total_attempts = MAX_RETRIES + 1;

		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let backoff = BACKOFF(attempt - 1);
				log::warn!(
					"seek in '{name}' to position {position}: retrying (attempt {}/{total_attempts}, waiting {backoff:?})",
					attempt + 1
				);
				thread::sleep(backoff);

				if let Err(e) = self.reconnect() {
					log::warn!(
						"seek in '{name}' to position {position}: reconnect failed (attempt {}/{total_attempts}): {e}",
						attempt + 1
					);
					if attempt >= MAX_RETRIES {
						return Err(e).with_context(|| {
							format!(
								"could not seek to position {position} in '{name}': reconnect failed — gave up after {total_attempts} attempts"
							)
						});
					}
					continue;
				}
			}

			match self.try_seek(position) {
				Ok(()) => return Ok(()),
				Err(e) if attempt < MAX_RETRIES => {
					log::warn!(
						"seek in '{name}' to position {position}: {e} (attempt {}/{total_attempts}), will retry",
						attempt + 1
					);
				}
				Err(e) => {
					return Err(e).with_context(|| {
						format!("could not seek to position {position} in '{name}' — gave up after {total_attempts} attempts")
					});
				}
			}
		}

		bail!("retry loop exited without returning — MAX_RETRIES invariant violated")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::anyhow;
	use std::collections::VecDeque;

	/// In-memory NetworkWriter stub. Each `try_*` / `reconnect` call pops the
	/// next programmed outcome from its queue; `()` means success.
	struct FakeWriter {
		name: String,
		position: u64,
		appended: Vec<u8>,
		written_at_start: Option<Vec<u8>>,
		append_outcomes: VecDeque<Result<()>>,
		write_at_outcomes: VecDeque<Result<()>>,
		seek_outcomes: VecDeque<Result<()>>,
		reconnect_outcomes: VecDeque<Result<()>>,
		append_calls: u32,
		write_at_calls: u32,
		seek_calls: u32,
		reconnect_calls: u32,
	}

	impl FakeWriter {
		fn new() -> Self {
			Self {
				name: "fake".to_string(),
				position: 0,
				appended: Vec::new(),
				written_at_start: None,
				append_outcomes: VecDeque::new(),
				write_at_outcomes: VecDeque::new(),
				seek_outcomes: VecDeque::new(),
				reconnect_outcomes: VecDeque::new(),
				append_calls: 0,
				write_at_calls: 0,
				seek_calls: 0,
				reconnect_calls: 0,
			}
		}
	}

	impl DataWriterTrait for FakeWriter {
		fn append(&mut self, _blob: &Blob) -> Result<ByteRange> {
			unreachable!("FakeWriter uses try_append via NetworkWriter default impl")
		}
		fn write_start(&mut self, _blob: &Blob) -> Result<()> {
			unreachable!("FakeWriter uses try_write_at via NetworkWriter default impl")
		}
		fn position(&mut self) -> Result<u64> {
			Ok(self.position)
		}
		fn set_position(&mut self, p: u64) -> Result<()> {
			self.position = p;
			Ok(())
		}
	}

	impl NetworkWriter for FakeWriter {
		fn try_append(&mut self, blob: &Blob) -> Result<ByteRange> {
			self.append_calls += 1;
			let outcome = self.append_outcomes.pop_front().unwrap_or(Ok(()));
			outcome?;
			let pos = self.position;
			self.appended.extend_from_slice(blob.as_slice());
			self.position += blob.len();
			Ok(ByteRange::new(pos, blob.len()))
		}
		fn try_write_at(&mut self, offset: u64, blob: &Blob, restore_pos: u64) -> Result<()> {
			self.write_at_calls += 1;
			let outcome = self.write_at_outcomes.pop_front().unwrap_or(Ok(()));
			outcome?;
			assert_eq!(offset, 0);
			self.written_at_start = Some(blob.as_slice().to_vec());
			self.position = restore_pos;
			Ok(())
		}
		fn try_seek(&mut self, position: u64) -> Result<()> {
			self.seek_calls += 1;
			let outcome = self.seek_outcomes.pop_front().unwrap_or(Ok(()));
			outcome?;
			self.position = position;
			Ok(())
		}
		fn reconnect(&mut self) -> Result<()> {
			self.reconnect_calls += 1;
			self.reconnect_outcomes.pop_front().unwrap_or(Ok(()))
		}
		fn writer_name(&self) -> &str {
			&self.name
		}
		fn tracked_position(&self) -> u64 {
			self.position
		}
	}

	// ── network_append ────────────────────────────────────────────────────────

	#[test]
	fn network_append_succeeds_on_first_attempt() {
		let mut w = FakeWriter::new();
		let range = w.network_append(&Blob::from(vec![1, 2, 3])).unwrap();
		assert_eq!(range, ByteRange::new(0, 3));
		assert_eq!(w.append_calls, 1);
		assert_eq!(w.reconnect_calls, 0);
		assert_eq!(w.appended, vec![1, 2, 3]);
	}

	#[test]
	fn network_append_recovers_on_retry() {
		let mut w = FakeWriter::new();
		w.append_outcomes.push_back(Err(anyhow!("transient")));
		let range = w.network_append(&Blob::from(vec![7, 8])).unwrap();
		assert_eq!(range, ByteRange::new(0, 2));
		assert_eq!(w.append_calls, 2);
		assert_eq!(w.reconnect_calls, 1); // one reconnect before attempt 1
	}

	#[test]
	fn network_append_gives_up_after_max_retries() {
		let mut w = FakeWriter::new();
		for _ in 0..=MAX_RETRIES {
			w.append_outcomes.push_back(Err(anyhow!("disk full")));
		}
		let err = w.network_append(&Blob::from(vec![1])).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("gave up"));
		assert!(msg.contains("disk full"));
		assert_eq!(w.append_calls, MAX_RETRIES + 1);
	}

	#[test]
	fn network_append_reconnect_failure_retries_until_exhaustion() {
		let mut w = FakeWriter::new();
		// Fail first attempt, then fail all reconnects.
		w.append_outcomes.push_back(Err(anyhow!("boom")));
		for _ in 0..=MAX_RETRIES {
			w.reconnect_outcomes.push_back(Err(anyhow!("link down")));
		}
		let err = w.network_append(&Blob::from(vec![1])).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("reconnect failed"));
		assert!(msg.contains("link down"));
	}

	#[test]
	fn network_append_reconnect_recovers_before_exhaustion() {
		let mut w = FakeWriter::new();
		// Attempt 0 fails; reconnect before attempt 1 fails too; reconnect before
		// attempt 2 succeeds and try_append then succeeds.
		w.append_outcomes.push_back(Err(anyhow!("boom")));
		w.reconnect_outcomes.push_back(Err(anyhow!("still down")));
		w.reconnect_outcomes.push_back(Ok(()));
		let range = w.network_append(&Blob::from(vec![9])).unwrap();
		assert_eq!(range.length, 1);
		assert_eq!(w.reconnect_calls, 2);
		// attempt 0: try_append (err); attempt 1: reconnect (err, continue) — no try_append;
		// attempt 2: reconnect (ok); try_append (ok) → 2 append calls total.
		assert_eq!(w.append_calls, 2);
	}

	// ── network_write_start ───────────────────────────────────────────────────

	#[test]
	fn network_write_start_succeeds_on_first_attempt() {
		let mut w = FakeWriter::new();
		w.position = 42; // should be restored after
		w.network_write_start(&Blob::from(vec![0xAA, 0xBB])).unwrap();
		assert_eq!(w.write_at_calls, 1);
		assert_eq!(w.written_at_start.unwrap(), vec![0xAA, 0xBB]);
		assert_eq!(w.position, 42);
	}

	#[test]
	fn network_write_start_recovers_on_retry() {
		let mut w = FakeWriter::new();
		w.write_at_outcomes.push_back(Err(anyhow!("transient")));
		w.network_write_start(&Blob::from(vec![1])).unwrap();
		assert_eq!(w.write_at_calls, 2);
	}

	#[test]
	fn network_write_start_gives_up_after_max_retries() {
		let mut w = FakeWriter::new();
		for _ in 0..=MAX_RETRIES {
			w.write_at_outcomes.push_back(Err(anyhow!("nope")));
		}
		let err = w.network_write_start(&Blob::from(vec![1])).unwrap_err();
		assert!(format!("{err:#}").contains("gave up"));
	}

	#[test]
	fn network_write_start_reconnect_failure_surfaces() {
		let mut w = FakeWriter::new();
		w.write_at_outcomes.push_back(Err(anyhow!("boom")));
		for _ in 0..=MAX_RETRIES {
			w.reconnect_outcomes.push_back(Err(anyhow!("link down")));
		}
		let err = w.network_write_start(&Blob::from(vec![1])).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("reconnect failed"));
	}

	// ── network_set_position ──────────────────────────────────────────────────

	#[test]
	fn network_set_position_succeeds_on_first_attempt() {
		let mut w = FakeWriter::new();
		w.network_set_position(123).unwrap();
		assert_eq!(w.position, 123);
		assert_eq!(w.seek_calls, 1);
		assert_eq!(w.reconnect_calls, 0);
	}

	#[test]
	fn network_set_position_recovers_on_retry() {
		let mut w = FakeWriter::new();
		w.seek_outcomes.push_back(Err(anyhow!("transient")));
		w.network_set_position(77).unwrap();
		assert_eq!(w.position, 77);
		assert_eq!(w.seek_calls, 2);
		assert_eq!(w.reconnect_calls, 1);
	}

	#[test]
	fn network_set_position_gives_up_after_max_retries() {
		let mut w = FakeWriter::new();
		for _ in 0..=MAX_RETRIES {
			w.seek_outcomes.push_back(Err(anyhow!("eof")));
		}
		let err = w.network_set_position(1).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("gave up"));
		assert!(msg.contains("eof"));
	}

	#[test]
	fn network_set_position_reconnect_failure_surfaces() {
		let mut w = FakeWriter::new();
		w.seek_outcomes.push_back(Err(anyhow!("boom")));
		for _ in 0..=MAX_RETRIES {
			w.reconnect_outcomes.push_back(Err(anyhow!("link down")));
		}
		let err = w.network_set_position(9).unwrap_err();
		assert!(format!("{err:#}").contains("reconnect failed"));
	}
}
