// Import necessary modules and traits
use super::{types::*, DataWriterFile, DataWriterTrait};
use crate::{
	containers::{TileConverterBox, TileConverterTrait, TileIterator, TileReaderBox},
	shared::{Blob, ProgressBar, Result, TileBBox, TileConverterConfig, TileCoord2},
};
use async_trait::async_trait;
use futures::executor::block_on;
use log::debug;
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
			writer: DataWriterFile::new(filename)?,
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
		self.writer.append(&blob)?;

		// Write metadata and blocks
		header.meta_range = self.write_meta(reader)?;
		header.blocks_range = self.write_blocks(reader)?;

		// Update the header and write it
		let blob: Blob = header.to_blob()?;
		self.writer.write_start(&blob)?;

		Ok(())
	}
}

// Implement additional methods for TileConverter
impl TileConverter {
	// Write metadata
	fn write_meta(&mut self, reader: &TileReaderBox) -> Result<ByteRange> {
		let meta = block_on(reader.get_meta())?;
		let compressed = self.config.get_compressor().process_blob(meta)?;

		self.writer.append(&compressed)
	}

	// Write blocks
	fn write_blocks(&mut self, reader: &mut TileReaderBox) -> Result<ByteRange> {
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
				let mut tiles_coverage = TileBBox::new(z, 0, 0, 255, 255);
				tiles_coverage.substract_coord2(&TileCoord2::new(x * 256, y * 256));
				tiles_coverage.intersect_bbox(&bbox_tiles.clone().substract_u32(x * 256, y * 256));

				blocks.push(BlockDefinition::new(x, y, z, tiles_coverage))
			}
		}

		// Initialize progress bar
		let sum = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut progress = ProgressBar::new("converting tiles", sum);

		// Create the block index
		let mut block_index = BlockIndex::new_empty();

		// Iterate through blocks and write them
		for mut block in blocks.into_iter() {
			let (tiles_range, index_range) = self.write_block(&block, reader, &mut progress)?;

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
		let range = self.writer.append(&block_index.as_brotli_blob())?;

		Ok(range)
	}

	// Write a single block
	fn write_block<'a>(
		&mut self, block: &BlockDefinition, reader: &'a mut TileReaderBox, progress: &mut ProgressBar,
	) -> Result<(ByteRange, ByteRange)> {
		// Log the start of the block
		debug!("start block {:?}", block);

		// Get the initial writer position
		let offset0 = self.writer.get_position()?;

		// Prepare the necessary data structures
		let bbox = block.get_global_bbox();
		println!("bbox {bbox:?}");
		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let mut tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		// Initialize mutexes for shared data structures
		//let mutex_progress = &Mutex::new(progress);
		//let mutex_writer = &Mutex::new(&mut self.writer);
		//let mutex_tile_index = &Mutex::new(&mut tile_index);
		//let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		// Create the tile converter and set parameters
		let tile_converter = self.config.get_tile_recompressor();

		// Acquire locks for shared data structures
		//let mut secured_tile_hash_lookup = mutex_tile_hash_lookup.lock()?;
		//let mut secured_tile_index = mutex_tile_index.lock()?;
		//let mut secured_writer = mutex_writer.lock()?;

		// Get the tile stream
		println!("A");
		let tile_iterator: TileIterator = reader.get_bbox_tile_iter(&bbox);

		//println!("B");
		//vec.sort_by_cached_key(|(coord, _blob)| coord.get_sort_index());
		//println!("C");

		// Compress the blobs if necessary
		if !tile_converter.is_empty() {
			//vec = tile_converter.process_vec(vec);
		}

		let mut i: u64 = 0;

		// Iterate through the blobs and process them
		for entry in tile_iterator {
			i += 1;

			let (coord, blob) = entry;
			//println!("coord {coord:?}");
			//vec = tile_converter.process_vec(vec); !!!!!
			//vec = tile_converter.process_vec(vec); !!!!!
			//vec = tile_converter.process_vec(vec); !!!!!
			//vec = tile_converter.process_vec(vec); !!!!!
			//vec = tile_converter.process_vec(vec); !!!!!

			//trace!("blob size {}", blob.len());

			let index = bbox.get_tile_index(&coord.as_coord2());

			let mut tile_hash_option = None;
			if blob.len() < 1000 {
				if tile_hash_lookup.contains_key(blob.as_slice()) {
					tile_index.set(index, *tile_hash_lookup.get(blob.as_slice()).unwrap());
					continue;
				}
				tile_hash_option = Some(blob.clone());
			}

			let mut range = self.writer.append(&blob)?;
			range.offset -= offset0;
			tile_index.set(index, range);

			if let Some(tile_hash) = tile_hash_option {
				tile_hash_lookup.insert(tile_hash.as_vec(), range);
			}

			if i > 256 {
				progress.inc(i);
				i = 0;
			}
		}

		// Increment progress and finish the row slice
		progress.inc(i);

		// Finish the block and write the index
		debug!("finish block and write index {:?}", block);

		let offset1 = self.writer.get_position()?;
		let index_range = self.writer.append(&tile_index.as_brotli_blob())?;

		Ok((ByteRange::new(offset0, offset1 - offset0), index_range))
	}
}
