// Import necessary modules and traits
use super::types::{BlockDefinition, BlockIndex, FileHeader, TileIndex};
#[cfg(feature = "full")]
use crate::helper::progress_bar::ProgressBar;
use crate::{
	container::{TilesReader, TilesStream, TilesWriter},
	helper::{compress, DataWriter, DataWriterFile},
	types::{Blob, ByteRange},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::{future::ready, StreamExt};
use log::{debug, trace};
#[cfg(feature = "full")]
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, path::Path};

// Define TilesWriter struct
pub struct VersaTilesWriter {
	writer: DataWriter,
}

impl VersaTilesWriter {
	// Create a new TilesWriter instance
	pub async fn open_path(path: &Path) -> Result<VersaTilesWriter>
	where
		Self: Sized,
	{
		Ok(VersaTilesWriter {
			writer: DataWriterFile::from_path(path)?,
		})
	}
}

// Implement TilesWriterTrait for TilesWriter
#[async_trait]
impl TilesWriter for VersaTilesWriter {
	// Convert tiles from the TilesReader
	async fn write_from_reader(&mut self, reader: &mut dyn TilesReader) -> Result<()> {
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

// Implement additional methods for TilesWriter
impl VersaTilesWriter {
	// Write metadata
	async fn write_meta(&mut self, reader: &dyn TilesReader) -> Result<ByteRange> {
		let meta: Blob = reader.get_meta()?.unwrap_or_default();
		let compressed = compress(meta, &reader.get_parameters().tile_compression)?;

		self.writer.append(&compressed)
	}

	// Write blocks
	async fn write_blocks(&mut self, reader: &mut dyn TilesReader) -> Result<ByteRange> {
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
		#[cfg(feature = "full")]
		let sum = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		#[cfg(feature = "full")]
		let progress = Arc::new(Mutex::new(ProgressBar::new("converting tiles", sum)));
		#[cfg(feature = "full")]
		let callback = |value| progress.clone().lock().unwrap().inc(value);
		#[cfg(not(feature = "full"))]
		let callback = |_value| ();

		// Create the block index
		let mut block_index = BlockIndex::new_empty();

		// Iterate through blocks and write them
		for mut block in blocks.into_iter() {
			let (tiles_range, index_range) = self.write_block(&block, reader, callback).await?;

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
		#[cfg(feature = "full")]
		progress.lock().unwrap().finish();

		let range = self.writer.append(&block_index.as_brotli_blob())?;

		Ok(range)
	}

	// Write a single block
	async fn write_block<'a, F>(
		&'a mut self, block: &BlockDefinition, reader: &'a mut dyn TilesReader, inc_progress: F,
	) -> Result<(ByteRange, ByteRange)>
	where
		F: Fn(u64),
	{
		// Log the start of the block
		debug!("start block {:?}", block);

		// Get the initial writer position
		let offset0 = self.writer.get_position()?;

		// Prepare the necessary data structures
		let bbox = &block.get_global_bbox().clone();

		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let mut tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		// Get the tile stream
		let tile_stream: TilesStream = reader.get_bbox_tile_stream(bbox).await;

		// Iterate through the blobs and process them
		tile_stream
			.for_each(|(coord, blob)| {
				inc_progress(1);

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
