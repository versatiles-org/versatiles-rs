use super::types::{BlockDefinition, BlockIndex, ByteRange, FileHeader, TileIndex};
use crate::opencloudtiles::{
	compress::compress_brotli,
	containers::abstract_container::{TileConverterTrait, TileReaderBox},
	progress::ProgressBar,
	types::{TileConverterConfig, TileCoord3},
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct TileConverter {
	file_buffer: BufWriter<File>,
	config: TileConverterConfig,
}

impl TileConverterTrait for TileConverter {
	fn new(filename: &PathBuf, tile_config: TileConverterConfig) -> Box<dyn TileConverterTrait>
	where
		Self: Sized,
	{
		let file = File::create(filename).unwrap();

		Box::new(TileConverter {
			file_buffer: BufWriter::new(file),
			config: tile_config,
		})
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		self.config.finalize_with_parameters(reader.get_parameters());

		let mut header = FileHeader::new(&self.config.get_tile_format());
		header.write(&mut self.file_buffer);

		header.meta_range = self.write_meta(&reader);
		header.blocks_range = self.write_blocks(reader);
		header.write(&mut self.file_buffer);
	}
}

impl TileConverter {
	fn write_meta(&mut self, reader: &TileReaderBox) -> ByteRange {
		let metablob = reader.get_meta().to_vec();
		let temp = compress_brotli(&metablob);
		return self.write(&temp);
	}
	fn write_blocks(&mut self, reader: &mut TileReaderBox) -> ByteRange {
		let zoom_range = self.config.get_zoom_range();
		let zoom_min = *zoom_range.start();
		let zoom_max = *zoom_range.end();

		let mut blocks: Vec<BlockDefinition> = Vec::new();

		let mut bar1 = ProgressBar::new("counting tiles", (zoom_max - zoom_min) as u64);

		for (index, bbox_tiles) in self.config.get_bbox_pyramide().iter().enumerate() {
			let zoom = index as u64;
			bar1.set_position((zoom - zoom_min) as u64);

			let bbox_blocks = bbox_tiles.clone().scale_down(256);
			for block in bbox_blocks.iter_tile_indexes() {
				blocks.push(BlockDefinition::new(
					zoom,
					block.x,
					block.y,
					bbox_tiles
						.clone()
						.clamped_offset_from(block.x * 256, block.y * 256),
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

		return self.write(&block_index.as_brotli_vec());
	}
	fn write_block(
		&mut self, block: &BlockDefinition, reader: &mut TileReaderBox, bar: &mut ProgressBar,
	) -> ByteRange {
		let bbox = &block.bbox;
		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let mutex_reader = &Mutex::new(reader);
		let mutex_writer = &Mutex::new(&mut self.file_buffer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		rayon::scope(|scope| {
			let mut tile_no: usize = 0;
			let tile_converter = self.config.get_tile_converter();

			for y_in_block in bbox.y_min..=bbox.y_max {
				for x_in_block in bbox.x_min..=bbox.x_max {
					bar.inc(1);

					let index = tile_no;
					tile_no += 1;

					let x = block.x * 256 + x_in_block;
					let y = block.y * 256 + y_in_block;

					let coord = TileCoord3 { x, y, z: block.level };

					scope.spawn(move |_s| {
						let optional_tile = mutex_reader.lock().unwrap().get_tile_data(&coord);

						if optional_tile.is_none() {
							let mut secured_writer = mutex_writer.lock().unwrap();
							let offset = secured_writer.stream_position().unwrap();
							let mut secured_tile_index = mutex_tile_index.lock().unwrap();
							secured_tile_index.set(index, ByteRange { offset, length: 0 });
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

						let mut secured_writer = mutex_writer.lock().unwrap();
						let range = ByteRange::new(
							secured_writer.stream_position().unwrap(),
							secured_writer.write(&result).unwrap() as u64,
						);
						drop(secured_writer);

						let mut secured_tile_index = mutex_tile_index.lock().unwrap();
						secured_tile_index.set(index, range.clone());
						drop(secured_tile_index);

						if secured_tile_hash_lookup.is_some() {
							secured_tile_hash_lookup
								.unwrap()
								.insert(tile_hash.unwrap(), range);
						}
					})
				}
			}
		});
		self.write(&tile_index.as_brotli_vec())
	}
	fn write(&mut self, buf: &[u8]) -> ByteRange {
		ByteRange::new(
			self
				.file_buffer
				.stream_position()
				.expect("Error in cloudtiles::write.stream_position"),
			self
				.file_buffer
				.write(buf)
				.expect("Error in cloudtiles::write.write") as u64,
		)
	}
}
