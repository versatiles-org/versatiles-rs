use super::types::{BlockDefinition, BlockIndex, ByteRange, CloudTilesDst, FileHeader, TileIndex};
use crate::opencloudtiles::{
	containers::abstract_container::{TileConverterTrait, TileReaderBox},
	helpers::{compress_brotli, ProgressBar},
	types::{TileConverterConfig, TileCoord2, TileCoord3},
};
use rayon::{iter::ParallelBridge, prelude::ParallelIterator};
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

pub struct TileConverter {
	writer: CloudTilesDst,
	config: TileConverterConfig,
}

impl TileConverterTrait for TileConverter {
	fn new(filename: &PathBuf, tile_config: TileConverterConfig) -> Box<dyn TileConverterTrait>
	where
		Self: Sized,
	{
		Box::new(TileConverter {
			writer: CloudTilesDst::new_file(filename),
			config: tile_config,
		})
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		self
			.config
			.finalize_with_parameters(reader.get_parameters());

		let mut header = FileHeader::new(&self.config.get_tile_format());
		self.writer.append(&header.to_bytes());

		header.meta_range = self.write_meta(&reader);
		header.blocks_range = self.write_blocks(reader);

		self.writer.write_start(&header.to_bytes())
	}
}

impl TileConverter {
	fn write_meta(&mut self, reader: &TileReaderBox) -> ByteRange {
		let metablob = reader.get_meta().to_vec();
		let temp = compress_brotli(&metablob);
		return self.writer.append(&temp);
	}
	fn write_blocks(&mut self, reader: &mut TileReaderBox) -> ByteRange {
		let mut blocks: Vec<BlockDefinition> = Vec::new();
		let mut bar1 = ProgressBar::new("counting tiles", self.config.get_max_zoom());

		for (zoom, bbox_tiles) in self.config.get_bbox_pyramide().iter_levels() {
			bar1.set_position(zoom);

			let bbox_blocks = bbox_tiles.clone().scale_down(256);
			for TileCoord2 { x, y } in bbox_blocks.iter_coords() {
				blocks.push(BlockDefinition::new(
					zoom,
					x,
					y,
					bbox_tiles.clone().clamped_offset_from(x * 256, y * 256),
				))
			}
		}
		bar1.finish();

		let sum = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut bar2 = ProgressBar::new("converting tiles", sum as u64);

		let mut block_index = BlockIndex::new_empty();

		for mut block in blocks.into_iter() {
			let range = self.write_block(&block, reader, &mut bar2);

			if range.length == 0 {
				// block is empty
				continue;
			}

			block.tile_range = range;
			block_index.add_block(block);
		}
		bar2.finish();

		return self.writer.append(&block_index.as_brotli_vec());
	}
	fn write_block(
		&mut self, block: &BlockDefinition, reader: &mut TileReaderBox, bar: &mut ProgressBar,
	) -> ByteRange {
		let bbox = &block.bbox;
		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let mutex_reader = &Mutex::new(reader);
		let mutex_bar = &Mutex::new(bar);
		let mutex_writer = &Mutex::new(&mut self.writer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		let tile_converter = self.config.get_tile_converter();

		bbox.iter_coords().par_bridge().for_each(|tile| {
			mutex_bar.lock().unwrap().inc(1);

			let index = bbox.get_tile_index(&tile);

			let x = block.x * 256 + tile.x;
			let y = block.y * 256 + tile.y;

			let coord = TileCoord3 {
				x,
				y,
				z: block.level,
			};

			let optional_tile = mutex_reader.lock().unwrap().get_tile_data(&coord);

			if optional_tile.is_none() {
				let mut secured_tile_index = mutex_tile_index.lock().unwrap();
				secured_tile_index.set(
					index,
					ByteRange {
						offset: 0,
						length: 0,
					},
				);
				return;
			}

			let tile = optional_tile.unwrap();

			let mut secured_tile_hash_lookup = None;
			let mut tile_hash = None;

			if tile.len() < 1000 {
				secured_tile_hash_lookup = Some(mutex_tile_hash_lookup.lock().unwrap());
				let lookup = secured_tile_hash_lookup.as_ref().unwrap();
				if lookup.contains_key(&tile) {
					let mut secured_tile_index = mutex_tile_index.lock().unwrap();
					secured_tile_index.set(index, lookup.get(&tile).unwrap().clone());
					return;
				}
				tile_hash = Some(tile.clone());
			}

			let result = tile_converter(&tile);

			let range = mutex_writer.lock().unwrap().append(&result);

			let mut secured_tile_index = mutex_tile_index.lock().unwrap();
			secured_tile_index.set(index, range.clone());
			drop(secured_tile_index);

			if secured_tile_hash_lookup.is_some() {
				secured_tile_hash_lookup
					.unwrap()
					.insert(tile_hash.unwrap(), range);
			}
		});

		return self.writer.append(&tile_index.as_brotli_vec());
	}
}
