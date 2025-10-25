//! This module provides functionality for writing tiles to `*.versatiles` containers.
//!
//! # Example
//!
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
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
//!     VersaTilesWriter::write_to_path(
//!         &mut reader,
//!         &path_out,
//!         TileCompression::Brotli,
//!         Config::default().arc()
//!     ).await?;
//!
//!     println!("Tiles have been successfully written to {path_out:?}");
//!
//!     Ok(())
//! }
//! ```

use std::sync::Arc;

use super::types::{BlockDefinition, BlockIndex, FileHeader};
use crate::{TilesReaderTrait, TilesReaderTraverseExt, TilesWriterTrait, container::versatiles::types::BlockWriter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::lock::Mutex;
use versatiles_core::{Traversal, config::Config, io::DataWriterTrait, types::*, utils::compress};
use versatiles_derive::context;

/// A struct for writing tiles to a VersaTiles container.
pub struct VersaTilesWriter {}

#[async_trait]
impl TilesWriterTrait for VersaTilesWriter {
	/// Convert tiles from the TilesReader and write them to the writer.
	async fn write_to_writer(
		reader: &mut dyn TilesReaderTrait,
		writer: &mut dyn DataWriterTrait,
		tile_compression: TileCompression,
		config: Arc<Config>,
	) -> Result<()> {
		// Finalize the configuration
		let parameters = reader.parameters();
		log::trace!("convert_from - reader.parameters: {parameters:?}");

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
		header.blocks_range = Self::write_blocks(reader, writer, tile_compression, config).await?;

		log::trace!("update header");
		let blob: Blob = header.to_blob()?;
		writer.write_start(&blob)?;

		Ok(())
	}
}

impl VersaTilesWriter {
	/// Write metadata to the writer.
	#[context("Failed to write metadata")]
	async fn write_meta(
		reader: &dyn TilesReaderTrait,
		writer: &mut dyn DataWriterTrait,
		compression: TileCompression,
	) -> Result<ByteRange> {
		let meta: Blob = reader.tilejson().into();
		let compressed = compress(meta, compression)?;

		writer.append(&compressed)
	}

	/// Write blocks to the writer.
	#[context("Failed to write blocks")]
	async fn write_blocks(
		reader: &mut dyn TilesReaderTrait,
		writer: &mut dyn DataWriterTrait,
		tile_compression: TileCompression,
		config: Arc<Config>,
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
									.write_tile(coord, tile.into_blob(tile_compression))
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
				config,
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
