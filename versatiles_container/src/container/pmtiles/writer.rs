//! Write tiles and metadata into a `PMTiles` v3 container.
//!
//! The `PMTilesWriter` produces a valid [PMTiles v3](https://github.com/protomaps/PMTiles) archive
//! from any [`TileSource`] source. It serializes tile data, compresses metadata and directories,
//! and builds a compact Hilbert-ordered directory layout.
//!
//! ## Behavior
//! - Compresses the metadata (`TileJSON`) and directory blocks with internal **gzip** compression.
//! - Stores tiles in **Hilbert order** for spatial locality.
//! - Uses `PMTiles` v3 header fields to describe data offsets and compression types.
//! - Produces a single binary blob that can be read back by [`PMTilesReader`](crate::container::pmtiles::PMTilesReader).
//!
//! ## Requirements
//! - The writer must output to a valid [`DataWriterTrait`] target (e.g. file, blob, memory).
//! - The input [`TileSource`] must provide consistent `tile_format` and `tile_compression`.
//!
//! ## Example
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let runtime = TilesRuntime::default();
//!
//!     // Open an existing MBTiles file
//!     let path = Path::new("/absolute/path/to/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(&path, runtime.clone())?;
//!
//!     // Convert it to PMTiles format
//!     let temp_path = std::env::temp_dir().join("berlin.pmtiles");
//!     PMTilesWriter::write_to_path(&mut reader, &temp_path, runtime).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors if writing, compression, or serialization fails.

use super::types::{EntriesV3, EntryV3, HeaderV3, PMTilesCompression};
use crate::{
	TileSource, TileSourceTraverseExt, TilesRuntime, TilesWriter,
	traversal::{Traversal, TraversalOrder},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::lock::Mutex;
use std::sync::Arc;
use versatiles_core::{
	compression::compress,
	io::DataWriterTrait,
	types::{Blob, ByteRange, TileCompression},
	utils::HilbertIndex,
};
use versatiles_derive::context;

/// Writer for `PMTiles` v3 archives.
///
/// Converts a [`TileSource`] source into a single `PMTiles` container by serializing
/// tiles, compressing metadata, generating directory entries, and writing a final header
/// with offsets and counts.
///
/// Tiles are ordered using the **Hilbert curve** to optimize spatial locality in the output.
pub struct PMTilesWriter {}

#[async_trait]
impl TilesWriter for PMTilesWriter {
	#[context("writing PMTiles to DataWriter")]
	/// Write tiles and metadata from a [`TileSource`] to a [`DataWriterTrait`] as a `PMTiles` archive.
	///
	/// This method:
	/// - Compresses metadata and directories with gzip (`INTERNAL_COMPRESSION`).
	/// - Orders tiles by Hilbert index to preserve spatial proximity.
	/// - Builds the `PMTiles` v3 header, directory blocks, and leaf entries.
	/// - Writes the final file in the correct binary layout (header + metadata + directories + tile data).
	///
	/// # Errors
	/// Returns an error if any I/O, compression, or serialization operation fails.
	async fn write_to_writer(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
	) -> Result<()> {
		const INTERNAL_COMPRESSION: TileCompression = TileCompression::Gzip;

		let parameters = reader.metadata().clone();

		let entries = EntriesV3::new();

		writer.set_position(16384)?;

		let mut header = HeaderV3::from_parameters(&parameters);

		let mut metadata: Blob = reader.tilejson().into();
		metadata = compress(metadata, INTERNAL_COMPRESSION)?;
		header.metadata = writer.append(&metadata)?;

		let tile_data_start = writer.get_position()?;

		let writer_mutex = Arc::new(Mutex::new(writer));
		let entries_mutex = Arc::new(Mutex::new(entries));
		let tile_compression = reader.metadata().tile_compression;

		reader
			.traverse_all_tiles(
				&Traversal::new(TraversalOrder::PMTiles, 1, 64)?,
				|_bbox, stream| {
					let writer_mutex = Arc::clone(&writer_mutex);
					let entries_mutex = Arc::clone(&entries_mutex);
					Box::pin(async move {
						let mut writer = writer_mutex.lock().await;
						let mut entries = entries_mutex.lock().await;
						let mut tiles = stream.to_vec().await;
						tiles.sort_by_key(|(coord, _)| coord.get_hilbert_index().unwrap());
						for (coord, mut tile) in tiles {
							let id = coord.get_hilbert_index()?;
							let range = writer.append(tile.as_blob(tile_compression)?)?;
							entries.push(EntryV3::new(id, range.shifted_backward(tile_data_start), 1));
						}
						Ok(())
					})
				},
				runtime.clone(),
				None,
			)
			.await?;

		let mut entries = entries_mutex.lock().await;
		let mut writer = writer_mutex.lock().await;

		let tile_data_end = writer.get_position()?;

		header.tile_data = ByteRange::new(tile_data_start, tile_data_end - tile_data_start);

		writer.set_position(HeaderV3::len())?;
		let directory = entries.as_directory(16384 - HeaderV3::len(), INTERNAL_COMPRESSION)?;
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
	use super::*;
	use crate::{
		TileSourceMetadata,
		container::{
			mock::{MockReader, MockWriter},
			pmtiles::PMTilesReader,
		},
	};
	use versatiles_core::{TileBBox, TileBBoxPyramid, TileFormat, io::*};
	use versatiles_derive::context;

	#[context("test: PMTiles read↔write roundtrip")]
	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_full_up_to(4),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
			traversal: Traversal::ANY,
		})?;

		let runtime = TilesRuntime::default();

		let mut data_writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(&mut mock_reader, &mut data_writer, runtime.clone()).await?;

		let data_reader = DataReaderBlob::from(data_writer);
		let mut reader = PMTilesReader::open_reader(Box::new(data_reader), runtime).await?;
		MockWriter::write(&mut reader).await?;

		Ok(())
	}

	#[context("test: PMTiles tile ordering (Hilbert & offsets)")]
	#[tokio::test]
	async fn tiles_written_in_order() -> Result<()> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.include_bbox(&TileBBox::from_min_and_max(15, 4090, 4090, 5000, 5000)?);
		bbox_pyramid.include_bbox(&TileBBox::from_min_and_max(14, 250, 250, 260, 260)?);

		let mut mock_reader = MockReader::new_mock(TileSourceMetadata {
			bbox_pyramid,
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::MVT,
			traversal: Traversal::ANY,
		})?;

		let runtime = TilesRuntime::default();

		let mut data_writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(&mut mock_reader, &mut data_writer, runtime.clone()).await?;

		let data_reader = DataReaderBlob::from(data_writer);
		let reader = PMTilesReader::open_reader(Box::new(data_reader), runtime).await?;

		let entries = reader.get_tile_entries()?;
		let entries = entries.iter().collect::<Vec<_>>();
		assert_eq!(entries.len(), 203);
		let mut tile_id = 0;
		let mut offset = 0;
		for entry in entries {
			assert!(entry.tile_id > tile_id, "Tile IDs are not in order");
			assert!(entry.range.offset >= offset, "Tile ranges are not in order");
			tile_id = entry.tile_id;
			offset = entry.range.offset + entry.range.length;
		}
		Ok(())
	}
}
