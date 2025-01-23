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

use super::types::{BlockDefinition, BlockIndex, FileHeader, TileIndex};
use crate::TilesWriterTrait;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{debug, trace};
use std::collections::HashMap;
use versatiles_core::{io::DataWriterTrait, progress::*, types::*, utils::compress};

/// A struct for writing tiles to a VersaTiles container.
pub struct VersaTilesWriter {}

#[async_trait]
impl TilesWriterTrait for VersaTilesWriter {
	/// Convert tiles from the TilesReader and write them to the writer.
	async fn write_to_writer(reader: &mut dyn TilesReaderTrait, writer: &mut dyn DataWriterTrait) -> Result<()> {
		// Finalize the configuration
		let parameters = reader.get_parameters();
		trace!("convert_from - reader.parameters: {parameters:?}");

		// Get the bounding box pyramid
		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();
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
		let meta: Blob = reader.get_tilejson().into();
		let compressed = compress(meta, &reader.get_parameters().tile_compression)?;

		writer.append(&compressed)
	}

	/// Write blocks to the writer.
	async fn write_blocks(reader: &mut dyn TilesReaderTrait, writer: &mut dyn DataWriterTrait) -> Result<ByteRange> {
		let pyramid = reader.get_parameters().bbox_pyramid.clone();

		if pyramid.is_empty() {
			return Ok(ByteRange::empty());
		}

		// Initialize blocks and populate them
		let blocks: Vec<BlockDefinition> = pyramid
			.iter_levels()
			.flat_map(|level_bbox| {
				level_bbox
					.iter_bbox_grid(256)
					.map(|bbox_block| BlockDefinition::new(&bbox_block))
			})
			.collect();

		// Initialize progress bar
		let mut progress = get_progress_bar(
			"converting tiles",
			blocks.iter().map(|block| block.count_tiles()).sum::<u64>(),
		);

		// Create the block index
		let mut block_index = BlockIndex::new_empty();
		let mut tiles_count = 0;

		// Iterate through blocks and write them
		for mut block in blocks.into_iter() {
			let (tiles_range, index_range) = Self::write_block(&block, reader, writer, &mut progress).await?;

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

	/// Write a single block to the writer.
	async fn write_block(
		block: &BlockDefinition,
		reader: &mut dyn TilesReaderTrait,
		writer: &mut dyn DataWriterTrait,
		progress: &mut Box<dyn ProgressTrait>,
	) -> Result<(ByteRange, ByteRange)> {
		// Log the start of the block
		debug!("start block {:?}", block);

		// Get the initial writer position
		let offset0 = writer.get_position()?;

		// Prepare the necessary data structures
		let bbox = &block.get_global_bbox().clone();

		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let mut tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		// Get the tile stream
		let tile_stream: TileStream = reader.get_bbox_tile_stream(bbox.clone()).await;

		// Iterate through the blobs and process them
		tile_stream
			.for_each_sync(|(coord, blob)| {
				progress.inc(1);

				let index = bbox.get_tile_index2(&coord.as_coord2()).unwrap();

				let mut save_hash = false;
				if blob.len() < 1000 {
					if let Some(range) = tile_hash_lookup.get(blob.as_slice()) {
						tile_index.set(index, *range);
						return;
					}
					save_hash = true;
				}

				let mut range = writer.append(&blob).unwrap();
				range.shift_backward(offset0);

				tile_index.set(index, range);

				if save_hash {
					tile_hash_lookup.insert(blob.into_vec(), range);
				}
			})
			.await;

		// Finish the block and write the index
		debug!("finish block and write index {:?}", block);

		// Get the final writer position
		let offset1 = writer.get_position()?;
		let index_range = writer.append(&tile_index.as_brotli_blob()?)?;

		Ok((ByteRange::new(offset0, offset1 - offset0), index_range))
	}
}
