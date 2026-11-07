//! A destination that discards every tile, for measuring a pipeline.
//!
//! Writing to `/dev/null` (or `NUL` on Windows) does everything a conversion to
//! `.versatiles` does except write: it reads the source in the same 256×256
//! blocks, and it encodes and compresses every tile, because a tile may still
//! hold an image that only becomes bytes there. What is left out is the disk,
//! so a run to `/dev/null` measures the source and the pipeline alone.

use anyhow::{Result, bail};
use versatiles_derive::context;

use crate::{TileSource, TileSourceTraverseExt, TilesRuntime, Traversal};

/// Names that mean "discard the output".
///
/// `/dev/null` is accepted on every platform, so that a script written for one
/// works on another.
#[must_use]
pub fn is_null_destination(destination: &str) -> bool {
	destination == "/dev/null" || (cfg!(windows) && destination.eq_ignore_ascii_case("NUL"))
}

/// Encodes every tile of a source and discards the result.
pub struct NullWriter;

impl NullWriter {
	/// Reads and encodes every tile of `reader`, and keeps none of them.
	///
	/// Fails, as a real destination would, when the source could not deliver
	/// some tiles.
	#[context("encoding tiles from '{}' and discarding them", reader.source_type())]
	pub async fn write(reader: &dyn TileSource, runtime: TilesRuntime) -> Result<()> {
		let tile_compression = *reader.metadata().tile_compression();
		reader
			.traverse_all_tiles(
				&Traversal::new_any_size(256, 256)?,
				move |_bbox, stream| {
					Box::pin(async move {
						let mut blobs = stream.map_parallel_try(move |_coord, tile| tile.into_blob(&tile_compression));
						while let Some((coord, blob)) = blobs.next().await {
							blob.with_context(|| format!("encoding tile {coord:?}"))?;
						}
						Ok(())
					})
				},
				runtime.clone(),
			)
			.await?;
		if runtime.had_errors() {
			bail!("the conversion completed with {} read error(s)", runtime.error_count());
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockReader, MockReaderProfile, TileSource};

	#[test]
	fn dev_null_means_discard_everywhere() {
		assert!(is_null_destination("/dev/null"));
		assert!(!is_null_destination("/dev/null2"));
		assert!(!is_null_destination("null.versatiles"));
		assert_eq!(is_null_destination("nul"), cfg!(windows));
	}

	#[tokio::test]
	async fn the_registry_sends_dev_null_here() -> Result<()> {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?.into_shared();
		TilesRuntime::default().write_to_str(reader, "/dev/null").await
	}

	#[tokio::test]
	async fn read_errors_fail_the_run() -> Result<()> {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Pbf)?;
		let runtime = TilesRuntime::default();
		// As the CLI runs it.
		runtime.set_abort_on_error(true);
		runtime.record_error("a tile", &anyhow::anyhow!("unreadable"));
		let error = NullWriter::write(&reader, runtime)
			.await
			.expect_err("an error was recorded");
		assert!(format!("{error:#}").contains("1 read error"), "{error:#}");
		Ok(())
	}
}
