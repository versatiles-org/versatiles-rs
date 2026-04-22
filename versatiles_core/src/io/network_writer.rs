use super::DataWriterTrait;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, bail};
use std::{thread, time::Duration};

const MAX_RETRIES: u32 = 2;

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
				let backoff = Duration::from_secs(1 << (attempt - 1));
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
				let backoff = Duration::from_secs(1 << (attempt - 1));
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
				let backoff = Duration::from_secs(1 << (attempt - 1));
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
