// Import necessary modules and traits
use super::{types::*, DataWriterFile, DataWriterTrait};
use crate::{
	containers::{TilesConverterBox, TilesConverterTrait, TilesReaderBox, TilesStream},
	shared::{Blob, ProgressBar, TileBBox, TilesConverterConfig},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::{future::ready, StreamExt};
use log::{debug, trace};
use std::{collections::HashMap, path::Path};

// Define TilesConverter struct
pub struct VersaTilesConverter {
	writer: Box<dyn DataWriterTrait>,
	config: TilesConverterConfig,
}

// Implement TilesConverterTrait for TilesConverter
#[async_trait]
impl TilesConverterTrait for VersaTilesConverter {
	// Create a new TilesConverter instance
	async fn open_file(path: &Path, config: TilesConverterConfig) -> Result<TilesConverterBox>
	where
		Self: Sized,
	{
		Ok(Box::new(VersaTilesConverter {
			writer: DataWriterFile::new(path)?,
			config,
		}))
	}

	// Convert tiles from the TilesReader
	async fn convert_from(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		// Finalize the configuration

		trace!("convert_from - self.config original: {:?}", self.config);

		let parameters = reader.get_parameters();
		trace!("convert_from - parameters: {parameters:?}");

		self.config.finalize_with_parameters(parameters);
		trace!("convert_from - self.config patched: {:?}", self.config);

		// Get the bounding box pyramid
		let bbox_pyramid = self.config.get_bbox_pyramid();
		trace!("convert_from - bbox_pyramid: {bbox_pyramid:#}");

		// Create the file header
		let mut header = FileHeader::new(
			self.config.get_tile_format(),
			self.config.get_tile_compression(),
			[
				bbox_pyramid.get_zoom_min().unwrap(),
				bbox_pyramid.get_zoom_max().unwrap(),
			],
			&bbox_pyramid.get_geo_bbox(),
		)?;

		// Convert the header to a blob and write it
		let blob: Blob = header.to_blob()?;
		trace!("write header");
		self.writer.append(&blob)?;

		trace!("write meta");
		header.meta_range = self.write_meta(reader).await?;

		trace!("write blocks");
		header.blocks_range = self.write_blocks(reader).await?;

		trace!("update header");
		let blob: Blob = header.to_blob()?;
		self.writer.write_start(&blob)?;

		Ok(())
	}
}

// Implement additional methods for TilesConverter
impl VersaTilesConverter {
	// Write metadata
	async fn write_meta(&mut self, reader: &TilesReaderBox) -> Result<ByteRange> {
		let meta: Blob = reader.get_meta().await?.unwrap_or_default();
		let compressed = self.config.get_compressor().process_blob(meta)?;

		self.writer.append(&compressed)
	}

	// Write blocks
	async fn write_blocks(&mut self, reader: &mut TilesReaderBox) -> Result<ByteRange> {
		let pyramid = self.config.get_bbox_pyramid();
		if pyramid.is_empty() {
			return Ok(ByteRange::empty());
		}

		// Initialize blocks and populate them
		let mut blocks: Vec<BlockDefinition> = Vec::new();
		for bbox_tiles in self.config.get_bbox_pyramid().iter_levels() {
			let mut bbox_blocks = *bbox_tiles;
			bbox_blocks.scale_down(256);

			for coord in bbox_blocks.iter_coords() {
				let x = coord.get_x() * 256;
				let y = coord.get_y() * 256;
				let level = coord.get_z();
				let size = 2u32.pow(level.min(8) as u32) - 1;

				let mut bbox_block = *bbox_tiles;
				bbox_block.intersect_bbox(&TileBBox::new(level, x, y, x + size, y + size)?);
				blocks.push(BlockDefinition::new(bbox_block))
			}
		}

		// Initialize progress bar
		let sum = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut progress = ProgressBar::new("converting tiles", sum);

		// Create the block index
		let mut block_index = BlockIndex::new_empty();

		// Iterate through blocks and write them
		for mut block in blocks.into_iter() {
			let (tiles_range, index_range) = self.write_block(&block, reader, &mut progress).await?;

			if tiles_range.length + index_range.length == 0 {
				// Block is empty, continue with the next block
				continue;
			}

			// Update the block with the tile and index range and add it to the block index
			block.set_tiles_range(tiles_range);
			block.set_index_range(index_range);
			block_index.add_block(block);
		}

		// Finish updating progress and write the block index
		progress.finish();
		let range = self.writer.append(&block_index.as_brotli_blob())?;

		Ok(range)
	}

	// Write a single block
	async fn write_block<'a>(
		&'a mut self, block: &BlockDefinition, reader: &'a mut TilesReaderBox, progress: &'a mut ProgressBar,
	) -> Result<(ByteRange, ByteRange)> {
		// Log the start of the block
		debug!("start block {:?}", block);

		// Get the initial writer position
		let offset0 = self.writer.get_position()?;

		// Prepare the necessary data structures
		let bbox = *block.get_global_bbox();

		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let mut tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		// Create the tile converter and set parameters
		let tile_converter = self.config.get_tile_recompressor();

		// Get the tile stream
		let mut tile_stream: TilesStream = reader.get_bbox_tile_stream(bbox).await;

		if !tile_converter.is_empty() {
			tile_stream = tile_converter.process_stream(tile_stream)
		}

		// Iterate through the blobs and process them
		tile_stream
			.for_each(|(coord, blob)| {
				progress.inc(1);

				let index = bbox.get_tile_index(&coord.as_coord2());

				let mut tile_hash_option = None;
				if blob.len() < 1000 {
					if tile_hash_lookup.contains_key(blob.as_slice()) {
						tile_index.set(index, *tile_hash_lookup.get(blob.as_slice()).unwrap());
						return ready(());
					}
					tile_hash_option = Some(blob.clone());
				}

				let mut range = self.writer.append(&blob).unwrap();
				range.offset -= offset0;
				tile_index.set(index, range);

				if let Some(tile_hash) = tile_hash_option {
					tile_hash_lookup.insert(tile_hash.as_vec(), range);
				}

				ready(())
			})
			.await;

		// Finish the block and write the index
		debug!("finish block and write index {:?}", block);

		//let mut writer = writer_mut.lock().await;
		//let mut writer = writer_mut1.lock().await;
		let offset1 = self.writer.get_position()?;
		let index_range = self.writer.append(&tile_index.as_brotli_blob())?;

		Ok((ByteRange::new(offset0, offset1 - offset0), index_range))
	}
}
