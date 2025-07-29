//! This module provides functionality for writing tiles to `*.versatiles` containers.
//!
//! # Example
//!
//! ```rust
//! use versatiles_container::{MBTilesReader, TilesWriterTrait, VersaTilesWriter};
//! use versatiles_core::types::{TileBBoxPyramid, TileCompression, TileFormat};
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create a mock tiles reader
//!     let path_in = std::env::current_dir()?.join("../testdata/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(&path_in)?;
//!
//!     // Specify the output path for the .versatiles file
//!     let path_out = std::env::current_dir()?.join("../testdata/temp5.versatiles");
//!
//!     // Write the tiles to the .versatiles file
//!     VersaTilesWriter::write_to_path(&mut reader, &path_out).await?;
//!
//!     println!("Tiles have been successfully written to {path_out:?}");
//!
//!     Ok(())
//! }
//! ```

use super::types::{BlockDefinition, BlockIndex, FileHeader};
use crate::{TilesWriterTrait, container::versatiles::types::BlockWriter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use log::{debug, trace};
use versatiles_core::{io::DataWriterTrait, progress::*, types::*, utils::compress};

/// A struct for writing tiles to a VersaTiles container.
pub struct VersaTilesWriter {}

#[async_trait]
impl TilesWriterTrait for VersaTilesWriter {
	/// Convert tiles from the TilesReader and write them to the writer.
	async fn write_to_writer(reader: &mut dyn TilesReaderTrait, writer: &mut dyn DataWriterTrait) -> Result<()> {
		// Finalize the configuration
		let parameters = reader.parameters();
		trace!("convert_from - reader.parameters: {parameters:?}");

		// Get the bounding box pyramid
		let bbox_pyramid = reader.parameters().bbox_pyramid.clone();
		trace!("convert_from - bbox_pyramid: {bbox_pyramid:#}");

		// Create the file header
		let mut header = FileHeader::new(
			&parameters.tile_format,
			&parameters.tile_compression,
			[
				bbox_pyramid.get_zoom_min().ok_or(anyhow!("invalid minzoom"))?,
				bbox_pyramid.get_zoom_max().ok_or(anyhow!("invalid maxzoom"))?,
			],
			&bbox_pyramid.get_geo_bbox().ok_or(anyhow!("invalid geo bounding box"))?,
		)?;

		// Convert the header to a blob and write it
		let blob: Blob = header.to_blob()?;
		trace!("write header");
		writer.append(&blob)?;

		trace!("write meta");
		header.meta_range = Self::write_meta(reader, writer).await?;

		trace!("write blocks");
		header.blocks_range = Self::write_blocks(reader, writer).await?;

		trace!("update header");
		let blob: Blob = header.to_blob()?;
		writer.write_start(&blob)?;

		Ok(())
	}
}

impl VersaTilesWriter {
	/// Write metadata to the writer.
	async fn write_meta(reader: &dyn TilesReaderTrait, writer: &mut dyn DataWriterTrait) -> Result<ByteRange> {
		let meta: Blob = reader.tilejson().into();
		let compressed = compress(meta, &reader.parameters().tile_compression)?;

		writer.append(&compressed)
	}

	/// Write blocks to the writer.
	async fn write_blocks(reader: &mut dyn TilesReaderTrait, writer: &mut dyn DataWriterTrait) -> Result<ByteRange> {
		let pyramid = reader.parameters().bbox_pyramid.clone();

		if pyramid.is_empty() {
			return Ok(ByteRange::empty());
		}

		// Initialize blocks and populate them
		use TraversalOrder::*;
		let block_defs: Vec<BlockDefinition> = reader
			.iter_bboxes_in_preferred_order(&[TopDown, BottomUp, DepthFirst256])?
			.flat_map(|level_bbox| {
				level_bbox
					.iter_bbox_grid(256)
					.map(|bbox_block| BlockDefinition::new(&bbox_block))
					.collect::<Vec<_>>()
			})
			.collect();

		// Initialize progress bar
		let mut progress = get_progress_bar(
			"converting tiles",
			block_defs.iter().map(|block| block.count_tiles()).sum::<u64>(),
		);

		// Create the block index
		let mut block_index = BlockIndex::new_empty();
		let mut tiles_count = 0;

		// Iterate through blocks and write them
		for mut block in block_defs.into_iter() {
			// Log the start of the block
			debug!("start block {block:?}");

			// Create a new BlockWriter for the block
			let mut block_writer = BlockWriter::new(&block, writer);

			reader
				.get_tile_stream(block_writer.bbox)
				.await?
				.for_each_sync(|(coord, blob)| {
					progress.inc(1);
					block_writer.write_tile(coord, blob).unwrap();
				})
				.await;

			// Finish the block
			debug!("finish block {block:?}");

			let (tiles_range, index_range) = block_writer.finalize()?;

			if tiles_range.length + index_range.length == 0 {
				// Block is empty, continue with the next block
				continue;
			}

			tiles_count += block.count_tiles();
			progress.set_position(tiles_count);

			// Update the block with the tile and index range and add it to the block index
			block.set_tiles_range(tiles_range);
			block.set_index_range(index_range);
			block_index.add_block(block);
		}

		// Finish updating progress and write the block index
		progress.finish();

		let range = writer.append(&block_index.as_brotli_blob()?)?;

		Ok(range)
	}
}
