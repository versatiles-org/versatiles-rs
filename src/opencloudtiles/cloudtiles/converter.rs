use crate::opencloudtiles::compress::compress_brotli;
use crate::opencloudtiles::{
	abstract_classes, progress::ProgressBar, TileFormat, TileReader, TileReaderWrapper,
};
use abstract_classes::TileConverterConfig;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::ops::Shr;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{BlockDefinition, BlockIndex, ByteRange, TileIndex};

pub struct TileConverter {
	file_buffer: BufWriter<File>,
	config: TileConverterConfig,
}

impl abstract_classes::TileConverter for TileConverter {
	fn new(
		filename: &PathBuf,
		tile_config: TileConverterConfig,
	) -> Result<Box<dyn abstract_classes::TileConverter>, &'static str>
	where
		Self: Sized,
	{
		let file = File::create(filename).expect("Unable to create file");
		Ok(Box::new(TileConverter {
			file_buffer: BufWriter::new(file),
			config: tile_config,
		}))
	}
	fn convert_from(&mut self, reader: Box<dyn TileReader>) -> Result<(), &'static str> {
		self
			.config
			.finalize_with_parameters(reader.get_parameters());

		self.write_header(&reader)?;
		let meta_range = self.write_meta(&reader)?;
		let blocks_range = self.write_blocks(&reader)?;
		self.update_header(&meta_range, &blocks_range)?;

		return Ok(());
	}
}

impl TileConverter {
	fn write_header(
		&mut self,
		reader: &Box<dyn abstract_classes::TileReader>,
	) -> Result<(), &'static str> {
		// magic word
		self.write(b"OpenCloudTiles/reader/v1   ")?;

		// tile format
		let tile_format = reader.get_parameters().get_tile_format();
		let tile_format_value: u8 = match tile_format {
			TileFormat::PNG => 0,
			TileFormat::JPG => 1,
			TileFormat::WEBP => 2,
			TileFormat::PBF | TileFormat::PBFGzip | TileFormat::PBFBrotli => 16,
		};
		self.write(&[tile_format_value])?;

		let tile_compression_value: u8 = match tile_format {
			TileFormat::PNG | TileFormat::JPG | TileFormat::WEBP | TileFormat::PBF => 0,
			TileFormat::PBFGzip => 1,
			TileFormat::PBFBrotli => 2,
		};
		self.write(&[tile_compression_value])?;

		// add zeros
		self.fill_with_zeros_till(256)?;

		return Ok(());
	}
	fn update_header(
		&mut self,
		meta_range: &ByteRange,
		blocks_range: &ByteRange,
	) -> Result<(), &'static str> {
		self.write_range_at(meta_range, 128)?;
		self.write_range_at(blocks_range, 144)?;

		return Ok(());
	}
	fn write_meta(
		&mut self,
		reader: &Box<dyn abstract_classes::TileReader>,
	) -> Result<ByteRange, &'static str> {
		let metablob = reader.get_meta().to_vec();
		return self.write_vec_brotli(&metablob);
	}
	fn write_blocks(
		&mut self,
		reader: &Box<dyn abstract_classes::TileReader>,
	) -> Result<ByteRange, &'static str> {
		let zoom_min = self.config.get_zoom_min();
		let zoom_max = self.config.get_zoom_max();

		let mut todos: Vec<BlockDefinition> = Vec::new();

		let mut bar1 = ProgressBar::new("counting tiles", zoom_max - zoom_min);

		for zoom in zoom_min..=zoom_max {
			bar1.set_position(zoom - zoom_min);

			let bbox = self.config.get_zoom_bbox(zoom).unwrap();

			let level_row_min: i64 = bbox.get_row_min() as i64;
			let level_row_max: i64 = bbox.get_row_max() as i64;
			let level_col_min: i64 = bbox.get_col_min() as i64;
			let level_col_max: i64 = bbox.get_col_max() as i64;

			for block_row in level_row_min.shr(8)..=level_row_max.shr(8) {
				for block_col in level_col_min.shr(8)..=level_col_max.shr(8) {
					let row0: i64 = block_row * 256i64;
					let col0: i64 = block_col * 256i64;

					let row_min = (level_row_min - row0).min(255).max(0) as u64;
					let row_max = (level_row_max - row0).min(255).max(0) as u64;
					let col_min = (level_col_min - col0).min(255).max(0) as u64;
					let col_max = (level_col_max - col0).min(255).max(0) as u64;

					todos.push(BlockDefinition {
						level: zoom,
						block_row: block_row as u64,
						block_col: block_col as u64,
						row_min,
						row_max,
						col_min,
						col_max,
						count: (row_max - row_min + 1) * (col_max - col_min + 1),
					})
				}
			}
		}
		bar1.finish();

		let sum = todos.iter().map(|x| x.count).sum();

		let mut bar2 = ProgressBar::new("converting tiles", sum);

		let mut index = BlockIndex::new();

		for todo in todos {
			let range = self.write_block(&todo, &reader, &mut bar2)?;

			if range.length == 0 {
				// block is empty
				continue;
			}

			index.add(&todo.level, &todo.block_row, &todo.block_col, &range)?;
		}
		bar2.finish();

		return self.write_vec_brotli(&index.as_vec());
	}
	fn write_block(
		&mut self,
		block: &BlockDefinition,
		reader: &Box<dyn abstract_classes::TileReader>,
		bar: &mut ProgressBar,
	) -> Result<ByteRange, &'static str> {
		let mut tile_index =
			TileIndex::new(block.row_min, block.row_max, block.col_min, block.col_max)?;
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let wrapped_reader = &TileReaderWrapper::new(reader);

		let mutex_writer = &Mutex::new(&mut self.file_buffer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		rayon::scope(|scope| {
			let mut tile_no: u64 = 0;
			let tile_converter = self.config.get_tile_converter();

			for row_in_block in block.row_min..=block.row_max {
				for col_in_block in block.col_min..=block.col_max {
					bar.inc(1);

					let index = tile_no;
					tile_no += 1;

					let row = block.block_row * 256 + row_in_block;
					let col = block.block_col * 256 + col_in_block;

					scope.spawn(move |_s| {
						let optional_tile = wrapped_reader.get_tile_raw(block.level, col, row);

						if optional_tile.is_none() {
							let mut secured_writer = mutex_writer.lock().unwrap();
							let offset = secured_writer.stream_position().unwrap();
							let mut secured_tile_index = mutex_tile_index.lock().unwrap();
							secured_tile_index
								.set(index, &ByteRange { offset, length: 0 })
								.unwrap();
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
								secured_tile_index
									.set(index, lookup.get(&tile).unwrap())
									.unwrap();
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
						secured_tile_index.set(index, &range).unwrap();
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
		self.write_vec_brotli(&tile_index.as_vec())
	}
	fn write(&mut self, buf: &[u8]) -> Result<ByteRange, &'static str> {
		Ok(ByteRange::new(
			self.file_buffer.stream_position().unwrap(),
			self.file_buffer.write(buf).unwrap() as u64,
		))
	}
	fn write_vec_brotli(&mut self, data: &Vec<u8>) -> Result<ByteRange, &'static str> {
		self.write(&compress_brotli(data))
	}
	fn write_range_at(&mut self, range: &ByteRange, pos: u64) -> Result<(), &'static str> {
		let current_pos = self.file_buffer.stream_position().unwrap();
		self
			.file_buffer
			.seek(std::io::SeekFrom::Start(pos))
			.unwrap();
		range.write_to(&mut self.file_buffer).unwrap();
		self
			.file_buffer
			.seek(std::io::SeekFrom::Start(current_pos))
			.unwrap();
		Ok(())
	}
	fn fill_with_zeros_till(&mut self, end_pos: u64) -> Result<ByteRange, &'static str> {
		let current_pos = self.file_buffer.stream_position().unwrap();
		if current_pos > end_pos {
			panic!("{} > {}", current_pos, end_pos);
		}
		let length = end_pos - current_pos;
		self.file_buffer.write(&vec![0; length as usize]).unwrap();
		Ok(ByteRange::new(current_pos, length))
	}
}
