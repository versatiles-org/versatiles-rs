use crate::opencloudtiles::{
	abstract_classes, progress::ProgressBar, Reader, ReaderWrapper, TileCompression, TileFormat,
};
use brotli::{enc::BrotliEncoderParams, BrotliCompress};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::ops::Shr;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{compress_brotli, compress_gzip, BlockDefinition, BlockIndex, ByteRange, TileIndex};

pub struct Converter {
	tile_compression: Option<TileCompression>,
	tile_recompress: bool,
	file_buffer: BufWriter<File>,
	minimum_zoom: Option<u64>,
	maximum_zoom: Option<u64>,
}

impl abstract_classes::Converter for Converter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn abstract_classes::Converter>>
	where
		Self: Sized,
	{
		let file = File::create(filename).expect("Unable to create file");
		Ok(Box::new(Converter {
			tile_compression: None,
			tile_recompress: false,
			file_buffer: BufWriter::new(file),
			minimum_zoom: None,
			maximum_zoom: None,
		}))
	}
	fn convert_from(&mut self, reader: Box<dyn Reader>) -> std::io::Result<()> {
		self.write_header(&reader)?;
		self.write_meta(&reader)?;
		self.write_blocks(&reader)?;

		return Ok(());
	}
	fn set_precompression(&mut self, compression: &TileCompression) {
		self.tile_compression = Some(compression.clone());
	}
	fn set_minimum_zoom(&mut self, level: u64) {
		self.minimum_zoom = Some(level);
	}
	fn set_maximum_zoom(&mut self, level: u64) {
		self.maximum_zoom = Some(level);
	}
}

impl Converter {
	fn write_header(&mut self, reader: &Box<dyn abstract_classes::Reader>) -> std::io::Result<()> {
		// magic word
		self.write(b"OpenCloudTiles/reader/v1   ")?;

		// tile format
		let tile_format = reader.get_tile_format();
		let tile_format_value: u8 = match tile_format {
			TileFormat::PNG => 0,
			TileFormat::JPG => 1,
			TileFormat::WEBP => 2,
			TileFormat::PBF => 16,
		};
		self.write(&[tile_format_value])?;

		// precompression
		let tile_compression_src = reader.get_tile_compression();

		if self.tile_compression.is_none() {
			self.tile_compression = Some(tile_compression_src.clone());
		}
		self.tile_recompress = self.tile_compression.as_ref().unwrap() != &tile_compression_src;

		let tile_compression_dst_value: u8 = match self.tile_compression {
			Some(TileCompression::None) => 0,
			Some(TileCompression::Gzip) => 1,
			Some(TileCompression::Brotli) => 2,
			None => panic!(),
		};
		self.write(&[tile_compression_dst_value])?;

		// println!("tile_compression: {:?}", self.tile_compression);
		// println!("tile_compression_src: {:?}", tile_compression_src);
		// println!("tile_recompress: {}", self.tile_recompress);

		// add zeros
		self.fill_with_zeros_till(256)?;

		return Ok(());
	}
	fn write_meta(&mut self, reader: &Box<dyn abstract_classes::Reader>) -> std::io::Result<()> {
		let metablob = reader.get_meta().to_vec();
		let meta_blob_range = self.write_vec_brotli(&metablob)?;
		let range = self.write_range_at(&meta_blob_range, 128)?;
		return Ok(range);
	}
	fn write_blocks(
		&mut self,
		reader: &Box<dyn abstract_classes::Reader>,
	) -> std::io::Result<ByteRange> {
		let mut level_min = reader.get_minimum_zoom();
		if self.minimum_zoom.is_some() {
			level_min = level_min.max(self.minimum_zoom.unwrap())
		}

		let mut level_max = reader.get_maximum_zoom();
		if self.maximum_zoom.is_some() {
			level_max = level_max.max(self.maximum_zoom.unwrap())
		}

		let mut todos: Vec<BlockDefinition> = Vec::new();

		let mut bar1 = ProgressBar::new("counting tiles", level_max - level_min);

		for level in level_min..=level_max {
			bar1.set_position(level - level_min);

			let bbox = reader.get_level_bbox(level);

			let level_row_min: i64 = bbox.0 as i64;
			let level_row_max: i64 = bbox.1 as i64;
			let level_col_min: i64 = bbox.2 as i64;
			let level_col_max: i64 = bbox.3 as i64;

			let block_row_min: i64 = level_row_min.shr(8);
			let block_row_max: i64 = level_row_max.shr(8);
			let block_col_min: i64 = level_col_min.shr(8);
			let block_col_max: i64 = level_col_max.shr(8);

			for block_row in block_row_min..=block_row_max {
				for block_col in block_col_min..=block_col_max {
					let row0 = (block_row * 256) as i64;
					let col0 = (block_col * 256) as i64;

					let row_min = (level_row_min - row0).min(255).max(0) as u64;
					let row_max = (level_row_max - row0).min(255).max(0) as u64;
					let col_min = (level_col_min - col0).min(255).max(0) as u64;
					let col_max = (level_col_max - col0).min(255).max(0) as u64;

					todos.push(BlockDefinition {
						level,
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

		let range = self.write_vec_brotli(&index.as_vec())?;
		return Ok(range);
	}
	fn write_block(
		&mut self,
		block: &BlockDefinition,
		reader: &Box<dyn abstract_classes::Reader>,
		bar: &mut ProgressBar,
	) -> std::io::Result<ByteRange> {
		let mut tile_index =
			TileIndex::new(block.row_min, block.row_max, block.col_min, block.col_max)?;
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let wrapped_reader = &ReaderWrapper::new(reader);

		let mutex_writer = &Mutex::new(&mut self.file_buffer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		let tile_recompress = &self.tile_recompress;
		let tile_compression = &self.tile_compression;

		rayon::scope(|scope| {
			let mut tile_no: u64 = 0;

			for row_in_block in block.row_min..=block.row_max {
				for col_in_block in block.col_min..=block.col_max {
					bar.inc(1);

					let index = tile_no;
					tile_no += 1;

					let row = block.block_row * 256 + row_in_block;
					let col = block.block_col * 256 + col_in_block;

					scope.spawn(move |_s| {
						let optional_tile = if *tile_recompress {
							wrapped_reader.get_tile_uncompressed(block.level, col, row)
						} else {
							wrapped_reader.get_tile_raw(block.level, col, row)
						};

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

						let result;
						if *tile_recompress {
							match tile_compression {
								Some(TileCompression::None) => result = tile,
								Some(TileCompression::Gzip) => {
									result = compress_gzip(&tile);
								}
								Some(TileCompression::Brotli) => {
									result = compress_brotli(&tile);
								}
								None => panic!(),
							}
						} else {
							result = tile;
						}

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
		let range = self.write_vec_brotli(&tile_index.as_vec())?;
		return Ok(range);
	}
	fn write(&mut self, buf: &[u8]) -> std::io::Result<ByteRange> {
		return Ok(ByteRange::new(
			self.file_buffer.stream_position()?,
			self.file_buffer.write(buf)? as u64,
		));
	}
	fn write_vec_brotli(&mut self, data: &Vec<u8>) -> std::io::Result<ByteRange> {
		let mut params = BrotliEncoderParams::default();
		params.quality = 11;
		params.size_hint = data.len();
		let mut cursor = Cursor::new(data);
		let offset = self.file_buffer.stream_position()?;
		let length = BrotliCompress(&mut cursor, &mut self.file_buffer, &params)? as u64;
		return Ok(ByteRange::new(offset, length));
	}
	fn write_range_at(&mut self, range: &ByteRange, pos: u64) -> std::io::Result<()> {
		let current_pos = self.file_buffer.stream_position()?;
		self.file_buffer.seek(std::io::SeekFrom::Start(pos))?;
		range.write_to(&mut self.file_buffer)?;
		self
			.file_buffer
			.seek(std::io::SeekFrom::Start(current_pos))?;
		return Ok(());
	}
	fn fill_with_zeros_till(&mut self, end_pos: u64) -> std::io::Result<ByteRange> {
		let current_pos = self.file_buffer.stream_position()?;
		if current_pos > end_pos {
			panic!("{} > {}", current_pos, end_pos);
		}
		let length = end_pos - current_pos;
		self.file_buffer.write(&vec![0; length as usize])?;
		return Ok(ByteRange::new(current_pos, length));
	}
}
