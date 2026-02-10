//! Write tiles and metadata into a `.versatiles` container.
//!
//! The `VersaTilesWriter` produces a valid `.versatiles` file from any [`TileSource`]
//! source. It serializes `TileJSON` metadata, groups tiles into fixed **256×256 blocks**,
//! compresses per-block tile indices and metadata, and writes a compact binary structure
//! ready for fast random access by the [`VersaTilesReader`](crate::container::versatiles::VersaTilesReader).
//!
//! ## File layout
//! ```notest
//! [ FileHeader | meta_blob | block_index_blob | blocks... ]
//! ```
//! Each block contains:
//! - a Brotli-compressed **tile index** (mapping tile IDs to byte ranges)
//! - a contiguous sequence of tile blobs in the reader’s `tile_format` and `tile_compression`
//!
//! ## Behavior
//! - All tiles are grouped in 256×256 blocks (`Traversal::new_any_size(256, 256)`).
//! - The header is written twice: once before, and once after writing metadata and blocks.
//! - Metadata (`TileJSON`) and block indices are compressed using Brotli for storage efficiency.
//! - The writer supports both raster and vector tile formats.
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
//!     // Open an MBTiles source
//!     let path_in = Path::new("../testdata/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(&path_in, runtime.clone())?;
//!
//!     // Write as a .versatiles container
//!     let path_out = std::env::temp_dir().join("berlin.versatiles");
//!     VersaTilesWriter::write_to_path(&mut reader, &path_out, runtime).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! Returns errors if writing fails, compression fails, or if metadata or bounding box
//! information is invalid.

use super::types::{BlockBuilder, BlockIndex, FileHeader};
use crate::{TileSource, TileSourceTraverseExt, TilesRuntime, TilesWriter, Traversal};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::lock::Mutex;
use std::sync::Arc;
use versatiles_core::{
	compression::compress,
	io::DataWriterTrait,
	types::{Blob, ByteRange, TileCompression},
};
use versatiles_derive::context;

/// Writer for `.versatiles` containers.
///
/// Serializes a [`TileSource`] source into a compact binary container optimized
/// for fast random tile access. The writer:
/// - compresses metadata and block indices with Brotli
/// - organizes tiles into 256×256 blocks
/// - writes the header twice to ensure integrity
///
/// The resulting file can be read by the [`VersaTilesReader`](crate::container::versatiles::VersaTilesReader).
pub struct VersaTilesWriter {}

#[async_trait]
impl TilesWriter for VersaTilesWriter {
	/// Convert tiles from a [`TileSource`] and write them to a [`DataWriterTrait`].
	///
	/// This method writes the file header, followed by metadata, blocks, and an updated
	/// header containing the final byte ranges. It compresses metadata and block indices
	/// using Brotli and enforces uniform tile format and compression across all tiles.
	///
	/// # Errors
	/// Returns an error if writing, compression, or bounding box validation fails.
	#[context("writing VersaTiles to DataWriter")]
	async fn write_to_writer(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
	) -> Result<()> {
		// Finalize the configuration
		let parameters = reader.metadata();
		log::trace!("convert_from - reader.parameters: {parameters:?}");

		let tile_compression = parameters.tile_compression;

		// Get the bounding box pyramid
		let bbox_pyramid = reader.metadata().bbox_pyramid.clone();
		log::trace!("convert_from - bbox_pyramid: {bbox_pyramid:#}");

		// Create the file header, preferring TileJSON values over pyramid-calculated ones
		let tilejson = reader.tilejson();
		let zoom_min = tilejson
			.min_zoom()
			.or(bbox_pyramid.get_level_min())
			.ok_or(anyhow!("invalid minzoom"))?;
		let zoom_max = tilejson
			.max_zoom()
			.or(bbox_pyramid.get_level_max())
			.ok_or(anyhow!("invalid maxzoom"))?;
		let bbox = tilejson
			.bounds
			.or(bbox_pyramid.get_geo_bbox())
			.ok_or(anyhow!("invalid geo bounding box"))?;
		let mut header = FileHeader::new(parameters.tile_format, tile_compression, [zoom_min, zoom_max], &bbox)?;

		// Convert the header to a blob and write it
		let blob: Blob = header.to_blob()?;
		log::trace!("write header");
		writer.append(&blob)?;

		log::trace!("write meta");
		header.meta_range = Self::write_meta(reader, writer, tile_compression).await?;

		log::trace!("write blocks");
		header.blocks_range = Self::write_blocks(reader, writer, tile_compression, runtime).await?;

		log::trace!("update header");
		let blob: Blob = header.to_blob()?;
		writer.write_start(&blob)?;

		Ok(())
	}
}

impl VersaTilesWriter {
	/// Write the `TileJSON` metadata as a Brotli-compressed blob to the writer.
	///
	/// Returns the byte range where the metadata was written.
	#[context("Failed to write metadata")]
	async fn write_meta(
		reader: &dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		compression: TileCompression,
	) -> Result<ByteRange> {
		let meta: Blob = reader.tilejson().into();
		let compressed = compress(meta, compression)?;

		writer.append(&compressed)
	}

	/// Write all tile blocks and their Brotli-compressed indices.
	///
	/// Traverses the reader in 256×256 blocks, writes tiles into each block, and appends
	/// the resulting block index at the end of the file.
	///
	/// Returns the byte range covering the block index blob.
	#[context("Failed to write blocks")]
	async fn write_blocks(
		reader: &mut dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		tile_compression: TileCompression,
		runtime: TilesRuntime,
	) -> Result<ByteRange> {
		if reader.metadata().bbox_pyramid.is_empty() {
			return Ok(ByteRange::empty());
		}

		// Create the block index
		let block_index_mutex = Arc::new(Mutex::new(BlockIndex::new_empty()));
		let writer_mutex = Arc::new(Mutex::new(writer));

		// Initialize blocks and populate them
		reader
			.traverse_all_tiles(
				&Traversal::new_any_size(256, 256)?,
				|bbox, stream| {
					let writer_mutex = Arc::clone(&writer_mutex);
					let block_index_mutex = Arc::clone(&block_index_mutex);

					Box::pin(async move {
						log::trace!("start processing block at {bbox:?}");

						// Compress tiles in parallel
						let compressed_stream = stream
							.map_parallel_try(move |_coord, tile| tile.into_blob(tile_compression))
							.unwrap_results();

						// Acquire writer lock and create block builder
						let mut writer = writer_mutex.lock().await;
						let mut block_builder = BlockBuilder::new(bbox.level, &mut **writer)?;

						// Stream compressed tiles to block builder
						compressed_stream
							.for_each(|coord, blob| {
								block_builder.write_tile(coord, blob).unwrap();
							})
							.await;

						// Finalize and add to block index if not empty
						if let Some(block) = block_builder.finalize()? {
							log::trace!("finish block {block:?}");
							block_index_mutex.lock().await.add_block(block);
						} else {
							log::trace!("skipping empty block at {bbox:?}");
						}

						Ok(())
					})
				},
				runtime.clone(),
				None,
			)
			.await?;

		// write the block index
		let range = writer_mutex
			.lock()
			.await
			.append(&block_index_mutex.lock().await.as_brotli_blob()?)?;

		Ok(range)
	}
}
