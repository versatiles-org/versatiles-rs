use crate::container::abstract_classes::{
	self, Reader, ReaderWrapper, TileCompression, TileFormat,
};
use brotli::{enc::BrotliEncoderParams, BrotliCompress};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::ops::Shr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use super::{compress_brotli, compress_gzip, BlockDefinition, BlockIndex, ByteRange, TileIndex};

pub struct Converter {
	tile_compression: Option<TileCompression>,
	tile_recompress: bool,
	file_buffer: BufWriter<File>,
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
		}))
	}
	fn convert_from(&mut self, container: Box<dyn Reader>) -> std::io::Result<()> {
		self.write_header(&container)?;
		self.write_meta(&container)?;
		self.write_blocks(&container)?;

		return Ok(());
	}
	fn set_precompression(&mut self, compression: &TileCompression) {
		self.tile_compression = Some(compression.clone());
	}
}

impl Converter {
	fn write_header(
		&mut self,
		container: &Box<dyn abstract_classes::Reader>,
	) -> std::io::Result<()> {
		// magic word
		self.write(b"OpenCloudTiles/Container/v1   ")?;

		// tile format
		let tile_format = container.get_tile_format();
		let tile_format_value: u8 = match tile_format {
			TileFormat::PNG => 0,
			TileFormat::JPG => 1,
			TileFormat::WEBP => 2,
			TileFormat::PBF => 16,
		};
		self.write(&[tile_format_value])?;

		// precompression
		let tile_compression_src = container.get_tile_compression();

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
	fn write_meta(&mut self, container: &Box<dyn abstract_classes::Reader>) -> std::io::Result<()> {
		let metablob = container.get_meta().to_vec();
		let meta_blob_range = self.write_vec_brotli(&metablob)?;
		let range = self.write_range_at(&meta_blob_range, 128)?;
		return Ok(range);
	}
	fn write_blocks(
		&mut self,
		container: &Box<dyn abstract_classes::Reader>,
	) -> std::io::Result<ByteRange> {
		let level_min = container.get_minimum_zoom();
		let level_max = container.get_maximum_zoom();

		let mut todos: Vec<BlockDefinition> = Vec::new();

		let bar1 = ProgressBar::new(level_max - level_min);
		bar1.set_style(
			ProgressStyle::with_template(
				"counting tiles: {wide_bar:0.white/dim.white} {pos:>9}/{len:9} {per_sec:18} {elapsed_precise} {eta_precise}",
			)
			.unwrap()
			.progress_chars("██▁"),
		);

		for level in level_min..=level_max {
			bar1.set_position(level - level_min);

			let bbox = container.get_level_bbox(level);

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
		bar1.abandon();

		let sum = todos.iter().map(|x| x.count).sum();

		let bar2 = ProgressBar::new(sum);
		bar2.set_style(
			ProgressStyle::with_template(
				"converting tiles: {wide_bar:0.white/dim.white} {pos:>9}/{len:9} {per_sec:18} {elapsed_precise} {eta_precise}",
			)
			.unwrap()
			.progress_chars("██▁"),
		);

		let mut index = BlockIndex::new();

		for todo in todos {
			let range = self.write_block(&todo, &container, &bar2)?;

			if range.length == 0 {
				// block is empty
				continue;
			}

			index.add(&todo.level, &todo.block_row, &todo.block_col, &range)?;
		}
		bar2.abandon();

		let range = self.write_vec_brotli(&index.as_vec())?;
		return Ok(range);
	}
	fn write_block(
		&mut self,
		block: &BlockDefinition,
		reader: &Box<dyn abstract_classes::Reader>,
		bar: &ProgressBar,
	) -> std::io::Result<ByteRange> {
		let mut tile_index =
			TileIndex::new(block.row_min, block.row_max, block.col_min, block.col_max)?;
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let wrapped_reader = ReaderWrapper::new(reader);

		let reader_mutex = Mutex::new(wrapped_reader);
		let writer_mutex = Mutex::new(&mut self.file_buffer);
		let tile_index_mutex = Mutex::new(&mut tile_index);
		let tile_hash_lookup_mutex = Mutex::new(tile_hash_lookup);

		let finalize_write = |compressed: &Vec<u8>, index: u64, tile_hash: Option<Vec<u8>>| {
			let mut save_writer = writer_mutex.lock().unwrap();
			let range = ByteRange::new(
				save_writer.stream_position().unwrap(),
				save_writer.write(compressed).unwrap() as u64,
			);
			drop(save_writer);

			let mut save_tile_index = tile_index_mutex.lock().unwrap();
			save_tile_index.set(index, &range).unwrap();
			drop(save_tile_index);

			if tile_hash.is_some() {
				let mut save_tile_hash_lookup = tile_hash_lookup_mutex.lock().unwrap();
				save_tile_hash_lookup.insert(tile_hash.unwrap(), range);
				drop(save_tile_hash_lookup);
			}
		};

		let write_raw = |tile: &Vec<u8>, index: u64, tile_hash: Option<Vec<u8>>| {
			finalize_write(&tile, index, tile_hash);
		};

		let write_gzip = move |tile: &Vec<u8>, index: u64, tile_hash: Option<Vec<u8>>| {
			let compressed = compress_gzip(tile);
			finalize_write(&compressed, index, tile_hash);
		};

		let write_brotli = move |tile: &Vec<u8>, index: u64, tile_hash: Option<Vec<u8>>| {
			let compressed = compress_brotli(tile);
			finalize_write(&compressed, index, tile_hash);
		};

		rayon::scope(|scope| {
			let save_reader = reader_mutex.lock().unwrap();

			let mut tile_no: u64 = 0;
			let mut progress_count = 0;
			let mut next_progress_update = SystemTime::now() + Duration::from_secs(10);

			for row_in_block in block.row_min..=block.row_max {
				for col_in_block in block.col_min..=block.col_max {
					progress_count += 1;
					if SystemTime::now() >= next_progress_update {
						next_progress_update = SystemTime::now() + Duration::from_secs(10);
						bar.inc(progress_count);
						progress_count = 0;
					}

					let index = tile_no;
					tile_no += 0;

					let row = block.block_row * 256 + row_in_block;
					let col = block.block_col * 256 + col_in_block;

					let optional_tile = if self.tile_recompress {
						save_reader.get_tile_uncompressed(block.level, col, row)
					} else {
						save_reader.get_tile_raw(block.level, col, row)
					};

					if optional_tile.is_none() {
						let mut save_write = writer_mutex.lock().unwrap();
						let offset = save_write.stream_position().unwrap();
						let mut save_tile_index = tile_index_mutex.lock().unwrap();
						save_tile_index
							.set(index, &ByteRange { offset, length: 0 })
							.unwrap();
						continue;
					}

					let tile = optional_tile.unwrap();

					let mut tile_hash: Option<Vec<u8>> = None;

					if tile.len() < 1000 {
						let save_tile_hash_lookup = tile_hash_lookup_mutex.lock().unwrap();
						if save_tile_hash_lookup.contains_key(&tile) {
							let mut save_tile_index = tile_index_mutex.lock().unwrap();
							save_tile_index
								.set(index, save_tile_hash_lookup.get(&tile).unwrap())
								.unwrap();
							continue;
						}
						tile_hash = Some(tile.clone());
					}

					if self.tile_recompress {
						match self.tile_compression {
							Some(TileCompression::None) => write_raw(&tile, index, tile_hash),
							Some(TileCompression::Gzip) => {
								if tile_hash.is_some() {
									write_gzip(&tile, index, tile_hash);
								} else {
									scope.spawn(move |_s| write_gzip(&tile, index, None));
								}
							}
							Some(TileCompression::Brotli) => {
								if tile_hash.is_some() {
									write_brotli(&tile, index, tile_hash);
								} else {
									scope.spawn(move |_s| write_brotli(&tile, index, None));
								}
							}
							None => panic!(),
						}
					} else {
						write_raw(&tile, index, tile_hash)
					}
				}
			}
			if progress_count > 0 {
				bar.inc(progress_count);
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
