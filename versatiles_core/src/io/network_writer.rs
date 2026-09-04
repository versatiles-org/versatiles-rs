use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use super::DataWriterTrait;
use crate::{Blob, ByteRange};

/// Trait for network-based data writers (SFTP, cloud storage, etc.)
/// that need retry logic with reconnection.
///
/// Implementors provide single-attempt operations and reconnection logic.
/// The default `network_*` methods handle retry loops with exponential backoff.
#[async_trait]
pub(crate) trait NetworkWriter: DataWriterTrait {
	/// Single-attempt write at current position.
	async fn try_append(&mut self, blob: &Blob) -> Result<ByteRange>;

	/// Single-attempt: seek to `offset`, write `blob`, seek back to `restore_pos`.
	async fn try_write_at(&mut self, offset: u64, blob: &Blob, restore_pos: u64) -> Result<()>;

	/// Single-attempt seek to `position`.
	async fn try_seek(&mut self, position: u64) -> Result<()>;

	/// Re-establish the connection (new session, reopen file, seek to tracked position).
	async fn reconnect(&mut self) -> Result<()>;

	/// Display name for log messages.
	fn writer_name(&self) -> &str;

	/// Current tracked write position (no I/O).
	fn tracked_position(&self) -> u64;

	/// Extra diagnostic context appended to write-failure warnings (e.g. connection
	/// age, bytes written, idle gap). Defaults to empty; implementors may override.
	fn failure_context(&self) -> String {
		String::new()
	}

	/// Append with retry and reconnect on failure.
	async fn network_append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let name = self.writer_name().to_string();
		let pos = self.tracked_position();
		let blob_len = blob.len();
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;

		for attempt in 0..=max_retries {
			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!(
					"write to '{name}' at position {pos}: retrying (attempt {}/{total_attempts}, waiting {backoff:?})",
					attempt + 1
				);
				tokio::time::sleep(backoff).await;

				if let Err(e) = self.reconnect().await {
					log::warn!(
						"write to '{name}' at position {pos}: reconnect failed (attempt {}/{total_attempts}): {e}",
						attempt + 1
					);
					if attempt >= max_retries {
						return Err(e).with_context(|| {
							format!("could not write {blob_len} bytes at position {pos} to '{name}': reconnect failed — gave up after {total_attempts} attempts")
						});
					}
					continue;
				}
			}

			match self.try_append(blob).await {
				Ok(range) => return Ok(range),
				Err(e) if attempt < max_retries => {
					let ctx = self.failure_context();
					log::warn!(
						"write to '{name}' at position {pos}: {e}{ctx} (attempt {}/{total_attempts}), will retry",
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
	async fn network_write_start(&mut self, blob: &Blob) -> Result<()> {
		let name = self.writer_name().to_string();
		let blob_len = blob.len();
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;

		for attempt in 0..=max_retries {
			let restore_pos = self.tracked_position();

			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!(
					"write_start to '{name}': retrying (attempt {}/{total_attempts}, waiting {backoff:?})",
					attempt + 1
				);
				tokio::time::sleep(backoff).await;

				if let Err(e) = self.reconnect().await {
					log::warn!(
						"write_start to '{name}': reconnect failed (attempt {}/{total_attempts}): {e}",
						attempt + 1
					);
					if attempt >= max_retries {
						return Err(e).with_context(|| {
							format!("could not write {blob_len} bytes at start of '{name}': reconnect failed — gave up after {total_attempts} attempts")
						});
					}
					continue;
				}
			}

			match self.try_write_at(0, blob, restore_pos).await {
				Ok(()) => return Ok(()),
				Err(e) if attempt < max_retries => {
					let ctx = self.failure_context();
					log::warn!(
						"write_start to '{name}': {e}{ctx} (attempt {}/{total_attempts}), will retry",
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
	async fn network_set_position(&mut self, position: u64) -> Result<()> {
		let name = self.writer_name().to_string();
		let policy = super::retry::policy();
		let max_retries = policy.max_retries;
		let total_attempts = max_retries + 1;

		for attempt in 0..=max_retries {
			if attempt > 0 {
				let backoff = policy.backoff(attempt - 1);
				log::warn!(
					"seek in '{name}' to position {position}: retrying (attempt {}/{total_attempts}, waiting {backoff:?})",
					attempt + 1
				);
				tokio::time::sleep(backoff).await;

				if let Err(e) = self.reconnect().await {
					log::warn!(
						"seek in '{name}' to position {position}: reconnect failed (attempt {}/{total_attempts}): {e}",
						attempt + 1
					);
					if attempt >= max_retries {
						return Err(e).with_context(|| {
							format!(
								"could not seek to position {position} in '{name}': reconnect failed — gave up after {total_attempts} attempts"
							)
						});
					}
					continue;
				}
			}

			match self.try_seek(position).await {
				Ok(()) => return Ok(()),
				Err(e) if attempt < max_retries => {
					let ctx = self.failure_context();
					log::warn!(
						"seek in '{name}' to position {position}: {e}{ctx} (attempt {}/{total_attempts}), will retry",
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
	use std::collections::VecDeque;

	use anyhow::anyhow;

	use super::*;

	/// Retry count from the shared policy (2 under `cfg(test)` defaults).
	fn max_retries() -> u32 {
		crate::io::retry::policy().max_retries
	}

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

	#[async_trait]
	impl DataWriterTrait for FakeWriter {
		async fn append(&mut self, _blob: &Blob) -> Result<ByteRange> {
			unreachable!("FakeWriter uses try_append via NetworkWriter default impl")
		}
		async fn write_start(&mut self, _blob: &Blob) -> Result<()> {
			unreachable!("FakeWriter uses try_write_at via NetworkWriter default impl")
		}
		async fn position(&mut self) -> Result<u64> {
			Ok(self.position)
		}
		async fn set_position(&mut self, p: u64) -> Result<()> {
			self.position = p;
			Ok(())
		}
	}

	#[async_trait]
	impl NetworkWriter for FakeWriter {
		async fn try_append(&mut self, blob: &Blob) -> Result<ByteRange> {
			self.append_calls += 1;
			let outcome = self.append_outcomes.pop_front().unwrap_or(Ok(()));
			outcome?;
			let pos = self.position;
			self.appended.extend_from_slice(blob.as_slice());
			self.position += blob.len();
			Ok(ByteRange::new(pos, blob.len()))
		}
		async fn try_write_at(&mut self, offset: u64, blob: &Blob, restore_pos: u64) -> Result<()> {
			self.write_at_calls += 1;
			let outcome = self.write_at_outcomes.pop_front().unwrap_or(Ok(()));
			outcome?;
			assert_eq!(offset, 0);
			self.written_at_start = Some(blob.as_slice().to_vec());
			self.position = restore_pos;
			Ok(())
		}
		async fn try_seek(&mut self, position: u64) -> Result<()> {
			self.seek_calls += 1;
			let outcome = self.seek_outcomes.pop_front().unwrap_or(Ok(()));
			outcome?;
			self.position = position;
			Ok(())
		}
		async fn reconnect(&mut self) -> Result<()> {
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

	#[tokio::test]
	async fn network_append_succeeds_on_first_attempt() {
		let mut w = FakeWriter::new();
		let range = w.network_append(&Blob::from(vec![1, 2, 3])).await.unwrap();
		assert_eq!(range, ByteRange::new(0, 3));
		assert_eq!(w.append_calls, 1);
		assert_eq!(w.reconnect_calls, 0);
		assert_eq!(w.appended, vec![1, 2, 3]);
	}

	#[tokio::test]
	async fn network_append_recovers_on_retry() {
		let mut w = FakeWriter::new();
		w.append_outcomes.push_back(Err(anyhow!("transient")));
		let range = w.network_append(&Blob::from(vec![7, 8])).await.unwrap();
		assert_eq!(range, ByteRange::new(0, 2));
		assert_eq!(w.append_calls, 2);
		assert_eq!(w.reconnect_calls, 1); // one reconnect before attempt 1
	}

	#[tokio::test]
	async fn network_append_gives_up_after_max_retries() {
		let mut w = FakeWriter::new();
		for _ in 0..=max_retries() {
			w.append_outcomes.push_back(Err(anyhow!("disk full")));
		}
		let err = w.network_append(&Blob::from(vec![1])).await.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("gave up"));
		assert!(msg.contains("disk full"));
		assert_eq!(w.append_calls, max_retries() + 1);
	}

	#[tokio::test]
	async fn network_append_reconnect_failure_retries_until_exhaustion() {
		let mut w = FakeWriter::new();
		// Fail first attempt, then fail all reconnects.
		w.append_outcomes.push_back(Err(anyhow!("boom")));
		for _ in 0..=max_retries() {
			w.reconnect_outcomes.push_back(Err(anyhow!("link down")));
		}
		let err = w.network_append(&Blob::from(vec![1])).await.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("reconnect failed"));
		assert!(msg.contains("link down"));
	}

	#[tokio::test]
	async fn network_append_reconnect_recovers_before_exhaustion() {
		let mut w = FakeWriter::new();
		// Attempt 0 fails; reconnect before attempt 1 fails too; reconnect before
		// attempt 2 succeeds and try_append then succeeds.
		w.append_outcomes.push_back(Err(anyhow!("boom")));
		w.reconnect_outcomes.push_back(Err(anyhow!("still down")));
		w.reconnect_outcomes.push_back(Ok(()));
		let range = w.network_append(&Blob::from(vec![9])).await.unwrap();
		assert_eq!(range.length, 1);
		assert_eq!(w.reconnect_calls, 2);
		// attempt 0: try_append (err); attempt 1: reconnect (err, continue) — no try_append;
		// attempt 2: reconnect (ok); try_append (ok) → 2 append calls total.
		assert_eq!(w.append_calls, 2);
	}

	// ── network_write_start ───────────────────────────────────────────────────

	#[tokio::test]
	async fn network_write_start_succeeds_on_first_attempt() {
		let mut w = FakeWriter::new();
		w.position = 42; // should be restored after
		w.network_write_start(&Blob::from(vec![0xAA, 0xBB])).await.unwrap();
		assert_eq!(w.write_at_calls, 1);
		assert_eq!(w.written_at_start.unwrap(), vec![0xAA, 0xBB]);
		assert_eq!(w.position, 42);
	}

	#[tokio::test]
	async fn network_write_start_recovers_on_retry() {
		let mut w = FakeWriter::new();
		w.write_at_outcomes.push_back(Err(anyhow!("transient")));
		w.network_write_start(&Blob::from(vec![1])).await.unwrap();
		assert_eq!(w.write_at_calls, 2);
	}

	#[tokio::test]
	async fn network_write_start_gives_up_after_max_retries() {
		let mut w = FakeWriter::new();
		for _ in 0..=max_retries() {
			w.write_at_outcomes.push_back(Err(anyhow!("nope")));
		}
		let err = w.network_write_start(&Blob::from(vec![1])).await.unwrap_err();
		assert!(format!("{err:#}").contains("gave up"));
	}

	#[tokio::test]
	async fn network_write_start_reconnect_failure_surfaces() {
		let mut w = FakeWriter::new();
		w.write_at_outcomes.push_back(Err(anyhow!("boom")));
		for _ in 0..=max_retries() {
			w.reconnect_outcomes.push_back(Err(anyhow!("link down")));
		}
		let err = w.network_write_start(&Blob::from(vec![1])).await.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("reconnect failed"));
	}

	// ── network_set_position ──────────────────────────────────────────────────

	#[tokio::test]
	async fn network_set_position_succeeds_on_first_attempt() {
		let mut w = FakeWriter::new();
		w.network_set_position(123).await.unwrap();
		assert_eq!(w.position, 123);
		assert_eq!(w.seek_calls, 1);
		assert_eq!(w.reconnect_calls, 0);
	}

	#[tokio::test]
	async fn network_set_position_recovers_on_retry() {
		let mut w = FakeWriter::new();
		w.seek_outcomes.push_back(Err(anyhow!("transient")));
		w.network_set_position(77).await.unwrap();
		assert_eq!(w.position, 77);
		assert_eq!(w.seek_calls, 2);
		assert_eq!(w.reconnect_calls, 1);
	}

	#[tokio::test]
	async fn network_set_position_gives_up_after_max_retries() {
		let mut w = FakeWriter::new();
		for _ in 0..=max_retries() {
			w.seek_outcomes.push_back(Err(anyhow!("eof")));
		}
		let err = w.network_set_position(1).await.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("gave up"));
		assert!(msg.contains("eof"));
	}

	#[tokio::test]
	async fn network_set_position_reconnect_failure_surfaces() {
		let mut w = FakeWriter::new();
		w.seek_outcomes.push_back(Err(anyhow!("boom")));
		for _ in 0..=max_retries() {
			w.reconnect_outcomes.push_back(Err(anyhow!("link down")));
		}
		let err = w.network_set_position(9).await.unwrap_err();
		assert!(format!("{err:#}").contains("reconnect failed"));
	}
}
