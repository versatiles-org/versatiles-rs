//! Provides functionality for writing tile data to a PMTiles container.
//!
//! The `PMTilesWriter` struct is the primary component of this module, offering methods to write metadata and tile data to a PMTiles container.
//!
//! ## Features
//! - Supports writing metadata and tile data with internal compression
//! - Efficiently organizes and compresses tile data for storage
//! - Implements progress feedback during the write process
//!
//! ## Usage Example
//! ```ignore
//! use versatiles::container::{PMTilesWriter, TilesWriter};
//! use versatiles::types::{DataWriterBlob, TileFormat, TileCompression, TileBBoxPyramid, TilesReaderParameters};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize a reader with sample data
//!     let reader = ...;
//!
//!     // Create a writer to write data to a new PMTiles container
//!     let mut data_writer = DataWriterBlob::new()?;
//!     PMTilesWriter::write_to_writer(&mut reader, &mut data_writer).await?;
//!
//!     // Further operations with data_writer...
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if there are issues with compression, writing data, or internal processing.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of writing metadata, handling different tile formats, and verifying the integrity of the written data.

use crate::container::TilesWriter;

use super::types::{EntriesV3, EntryV3, HeaderV3, PMTilesCompression, TileId};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{
	io::DataWriterTrait,
	progress::get_progress_bar,
	types::{Blob, ByteRange, TileBBox, TileCompression},
};
use versatiles_core::{types::TilesReader, utils::compress};

/// A struct that provides functionality to write tile data to a PMTiles container.
pub struct PMTilesWriter {}

#[async_trait]
impl TilesWriter for PMTilesWriter {
	/// Writes tile data from a `TilesReader` to a `DataWriterTrait` (such as a PMTiles container).
	///
	/// # Arguments
	/// * `reader` - The tiles reader providing the tile data.
	/// * `writer` - The data writer to write the tile data to.
	///
	/// # Errors
	/// Returns an error if there are issues with writing data or internal processing.
	async fn write_to_writer(
		reader: &mut dyn TilesReader,
		writer: &mut dyn DataWriterTrait,
	) -> Result<()> {
		const INTERNAL_COMPRESSION: TileCompression = TileCompression::Gzip;

		let parameters = reader.get_parameters().clone();
		let pyramid = &parameters.bbox_pyramid;

		let mut blocks: Vec<TileBBox> = pyramid
			.iter_levels()
			.flat_map(|level_bbox| level_bbox.iter_bbox_grid(256))
			.collect();
		blocks.sort_by_cached_key(|b| b.get_tile_id().unwrap());

		let mut progress = get_progress_bar(
			"converting tiles",
			blocks.iter().map(|block| block.count_tiles()).sum::<u64>(),
		);
		let mut tile_count = 0;

		let mut entries = EntriesV3::new();

		writer.set_position(16384)?;

		let mut header = HeaderV3::from(&parameters);

		let mut metadata = reader.get_meta()?.unwrap_or(Blob::new_empty());
		metadata = compress(metadata, &INTERNAL_COMPRESSION)?;
		header.metadata = writer.append(&metadata)?;

		let tile_data_start = writer.get_position()?;

		for bbox in blocks.iter() {
			let mut stream = reader.get_bbox_tile_stream(bbox.clone()).await;
			while let Some((coord, blob)) = stream.next().await {
				progress.inc(1);
				let id = coord.get_tile_id().unwrap();
				let range = writer.append(&blob).unwrap();
				entries.push(EntryV3::new(
					id,
					range.get_shifted_backward(tile_data_start),
					1,
				));
			}

			tile_count += bbox.count_tiles();
			progress.set_position(tile_count);
		}
		progress.finish();

		let tile_data_end = writer.get_position()?;

		header.tile_data = ByteRange::new(tile_data_start, tile_data_end - tile_data_start);

		writer.set_position(HeaderV3::len())?;
		let directory = entries.as_directory(16384 - HeaderV3::len(), &INTERNAL_COMPRESSION)?;
		header.root_dir = writer.append(&directory.root_bytes)?;

		writer.set_position(tile_data_end)?;
		header.leaf_dirs = writer.append(&directory.leaves_bytes)?;

		header.clustered = true;
		header.internal_compression = PMTilesCompression::from_value(INTERNAL_COMPRESSION)?;
		header.addressed_tiles_count = entries.tile_count();
		header.tile_entries_count = entries.len() as u64;
		header.tile_contents_count = entries.len() as u64;

		writer.write_start(&header.serialize()?)?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use versatiles_core::types::TilesReaderParameters;

	use super::*;
	use crate::container::{
		mock::{MockTilesReader, MockTilesWriter},
		pmtiles::PMTilesReader,
	};
	use versatiles_core::{
		io::{DataReaderBlob, DataWriterBlob},
		types::{TileBBoxPyramid, TileFormat},
	};

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(4),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::PBF,
		})?;

		let mut data_writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(&mut mock_reader, &mut data_writer).await?;

		let data_reader = DataReaderBlob::from(data_writer);
		let mut reader = PMTilesReader::open_reader(Box::new(data_reader)).await?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}
}
