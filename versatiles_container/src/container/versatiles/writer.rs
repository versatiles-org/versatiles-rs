//! Write tiles and metadata into a `.versatiles` container.
//!
//! The `VersaTilesWriter` produces a valid `.versatiles` file from any [`TileSourceTrait`]
//! source. It serializes TileJSON metadata, groups tiles into fixed **256×256 blocks**,
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

use super::types::{BlockDefinition, BlockIndex, FileHeader};
use crate::{
	TileSourceTrait, TilesReaderTraverseExt, TilesRuntime, TilesWriterTrait, Traversal,
	container::versatiles::types::BlockWriter,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::lock::Mutex;
use std::sync::Arc;
use versatiles_core::{io::DataWriterTrait, types::*, utils::compress};
use versatiles_derive::context;

/// Writer for `.versatiles` containers.
///
/// Serializes a [`TileSourceTrait`] source into a compact binary container optimized
/// for fast random tile access. The writer:
/// - compresses metadata and block indices with Brotli
/// - organizes tiles into 256×256 blocks
/// - writes the header twice to ensure integrity
///
/// The resulting file can be read by the [`VersaTilesReader`](crate::container::versatiles::VersaTilesReader).
pub struct VersaTilesWriter {}

#[async_trait]
impl TilesWriterTrait for VersaTilesWriter {
	/// Convert tiles from a [`TileSourceTrait`] and write them to a [`DataWriterTrait`].
	///
	/// This method writes the file header, followed by metadata, blocks, and an updated
	/// header containing the final byte ranges. It compresses metadata and block indices
	/// using Brotli and enforces uniform tile format and compression across all tiles.
	///
	/// # Errors
	/// Returns an error if writing, compression, or bounding box validation fails.
	#[context("writing VersaTiles to DataWriter")]
	async fn write_to_writer(
		reader: &mut dyn TileSourceTrait,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
	) -> Result<()> {
		// Finalize the configuration
		let parameters = reader.parameters();
		log::trace!("convert_from - reader.parameters: {parameters:?}");

		let tile_compression = parameters.tile_compression;

		// Get the bounding box pyramid
		let bbox_pyramid = reader.parameters().bbox_pyramid.clone();
		log::trace!("convert_from - bbox_pyramid: {bbox_pyramid:#}");

		// Create the file header
		let mut header = FileHeader::new(
			parameters.tile_format,
			tile_compression,
			[
				bbox_pyramid.get_level_min().ok_or(anyhow!("invalid minzoom"))?,
				bbox_pyramid.get_level_max().ok_or(anyhow!("invalid maxzoom"))?,
			],
			&bbox_pyramid.get_geo_bbox().ok_or(anyhow!("invalid geo bounding box"))?,
		)?;

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
	/// Write the TileJSON metadata as a Brotli-compressed blob to the writer.
	///
	/// Returns the byte range where the metadata was written.
	#[context("Failed to write metadata")]
	async fn write_meta(
		reader: &dyn TileSourceTrait,
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
		reader: &mut dyn TileSourceTrait,
		writer: &mut dyn DataWriterTrait,
		tile_compression: TileCompression,
		runtime: TilesRuntime,
	) -> Result<ByteRange> {
		if reader.parameters().bbox_pyramid.is_empty() {
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
						// Log the start of the block
						let mut block = BlockDefinition::new(&bbox).unwrap();
						log::trace!("start block {block:?}");

						// Create a new BlockWriter for the block
						let mut writer = writer_mutex.lock().await;
						let mut block_writer = BlockWriter::new(&block, &mut **writer);
						stream
							.for_each_sync(|(coord, tile)| {
								block_writer
									.write_tile(coord, tile.into_blob(tile_compression).unwrap())
									.unwrap();
							})
							.await;

						// Finish the block
						log::trace!("finish block {block:?}");

						let (tiles_range, index_range) = block_writer.finalize()?;

						if tiles_range.length + index_range.length == 0 {
							// Block is empty, continue with the next block
							return Ok(());
						}

						// Update the block with the tile and index range and add it to the block index
						block.set_tiles_range(tiles_range);
						block.set_index_range(index_range);
						block_index_mutex.lock().await.add_block(block);

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
