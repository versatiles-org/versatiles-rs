// Import necessary modules and traits
use super::{types::*, DataWriterFile, DataWriterTrait};
use crate::{
	containers::{TileConverterBox, TileConverterTrait, TileReaderBox},
	shared::{Blob, ProgressBar, Result, TileBBox, TileConverterConfig},
};
use async_trait::async_trait;
use futures::lock::Mutex;
use log::{debug, trace};
use std::collections::HashMap;

// Define TileConverter struct
pub struct TileConverter {
	writer: Box<dyn DataWriterTrait>,
	config: TileConverterConfig,
}

// Implement TileConverterTrait for TileConverter
#[async_trait]
impl TileConverterTrait for TileConverter {
	// Create a new TileConverter instance
	async fn new(filename: &str, tile_config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized,
	{
		Ok(Box::new(TileConverter {
			writer: DataWriterFile::new(filename).await?,
			config: tile_config,
		}))
	}

	// Convert tiles from the TileReader
	async fn convert_from(&mut self, reader: &mut TileReaderBox) -> Result<()> {
		// Finalize the configuration
		self.config.finalize_with_parameters(reader.get_parameters()?);

		// Get the bounding box pyramid
		let bbox_pyramid = self.config.get_bbox_pyramid();

		// Create the file header
		let mut header = FileHeader::new(
			self.config.get_tile_format(),
			self.config.get_tile_compression(),
			[
				bbox_pyramid.get_zoom_min().unwrap(),
				bbox_pyramid.get_zoom_max().unwrap(),
			],
			bbox_pyramid.get_geo_bbox(),
		);

		// Convert the header to a blob and write it
		let blob: Blob = header.to_blob()?;
		self.writer.append(&blob).await?;

		// Write metadata and blocks
		header.meta_range = self.write_meta(reader).await?;
		header.blocks_range = self.write_blocks(reader).await?;

		// Update the header and write it
		let blob: Blob = header.to_blob()?;
		self.writer.write_start(&blob).await?;

		Ok(())
	}
}

// Implement additional methods for TileConverter
impl TileConverter {
	// Write metadata
	async fn write_meta(&mut self, reader: &TileReaderBox) -> Result<ByteRange> {
		let meta = reader.get_meta().await?;
		let compressed = self.config.get_compressor().process_blob(meta)?;

		self.writer.append(&compressed).await
	}

	// Write blocks
	async fn write_blocks(&mut self, reader: &mut TileReaderBox) -> Result<ByteRange> {
		let pyramid = self.config.get_bbox_pyramid();
		if pyramid.is_empty() {
			return Ok(ByteRange::empty());
		}

		// Initialize blocks and populate them
		let mut blocks: Vec<BlockDefinition> = Vec::new();
		for bbox_tiles in self.config.get_bbox_pyramid().iter_levels() {
			let bbox_blocks = bbox_tiles.scale_down(256);
			for coord in bbox_blocks.iter_coords() {
				let x = coord.get_x();
				let y = coord.get_y();
				let z = coord.get_z();
				let mut bbox_block = *bbox_tiles;
				bbox_block.intersect_bbox(&TileBBox::new(z, x * 256, y * 256, x * 256 + 255, y * 256 + 255));

				blocks.push(BlockDefinition::new(x, y, z, bbox_block))
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
			block = block.with_tiles_range(tiles_range).with_index_range(index_range);
			block_index.add_block(block);
		}

		// Finish updating progress and write the block index
		progress.finish();
		let range = self.writer.append(&block_index.as_brotli_blob()).await?;

		Ok(range)
	}

	// Write a single block
	async fn write_block<'a>(
		&mut self, block: &BlockDefinition, reader: &'a mut TileReaderBox, progress: &mut ProgressBar,
	) -> Result<(ByteRange, ByteRange)> {
		// Log the start of the block
		debug!("start block {:?}", block);

		// Get the initial writer position
		let offset0 = self.writer.get_position().await.unwrap();

		// Prepare the necessary data structures
		let bbox = block.get_bbox();
		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		// Initialize mutexes for shared data structures
		let mutex_progress = &Mutex::new(progress);
		let mutex_writer = &Mutex::new(&mut self.writer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		// Create the tile converter and set parameters
		let tile_converter = self.config.get_tile_recompressor();

		// Acquire locks for shared data structures
		let mut secured_tile_hash_lookup = mutex_tile_hash_lookup.lock().await;
		let mut secured_tile_index = mutex_tile_index.lock().await;
		let mut secured_writer = mutex_writer.lock().await;

		// Get the tile stream
		let mut vec = reader.get_bbox_tile_vec(bbox).await?;

		vec.sort_by_cached_key(|(coord, _blob)| coord.get_sort_index());

		// Compress the blobs if necessary
		if !tile_converter.is_empty() {
			vec = tile_converter.process_vec(vec);
		}

		// Iterate through the blobs and process them
		for (coord, blob) in vec {
			trace!("blob size {}", blob.len());

			let index = bbox.get_tile_index(&coord.as_coord2());

			let mut tile_hash_option = None;
			if blob.len() < 1000 {
				if secured_tile_hash_lookup.contains_key(blob.as_slice()) {
					secured_tile_index.set(index, *secured_tile_hash_lookup.get(blob.as_slice()).unwrap());
					continue;
				}
				tile_hash_option = Some(blob.clone());
			}

			let mut range = secured_writer.append(&blob).await.unwrap();
			range.offset -= offset0;
			secured_tile_index.set(index, range);

			if let Some(tile_hash) = tile_hash_option {
				secured_tile_hash_lookup.insert(tile_hash.as_vec(), range);
			}
		}

		drop(secured_writer);
		drop(secured_tile_index);

		// Increment progress and finish the row slice
		mutex_progress.lock().await.inc(bbox.count_tiles());

		// Finish the block and write the index
		debug!("finish block and write index {:?}", block);

		let offset1 = self.writer.get_position().await.unwrap();
		let index_range = self.writer.append(&tile_index.as_brotli_blob()).await.unwrap();

		Ok((ByteRange::new(offset0, offset1 - offset0), index_range))
	}
}
