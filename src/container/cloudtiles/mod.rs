use super::container::{self, ReaderWrapper};
use super::container::{TileCompression, TileFormat};
use brotli::{enc::BrotliEncoderParams, BrotliCompress};
use flate2::{bufread::GzEncoder, Compression};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::ops::Shr;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Reader;
impl container::Reader for Reader {}

pub struct Converter {
	tile_compression: Option<TileCompression>,
	tile_recompress: bool,
	file_buffer: BufWriter<File>,
}
impl container::Converter for Converter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn container::Converter>>
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
	fn convert_from(&mut self, container: Box<dyn container::Reader>) -> std::io::Result<()> {
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
	fn write_header(&mut self, container: &Box<dyn container::Reader>) -> std::io::Result<()> {
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
	fn write_meta(&mut self, container: &Box<dyn container::Reader>) -> std::io::Result<()> {
		let metablob = container.get_meta().to_vec();
		let meta_blob_range = self.write_vec_brotli(&metablob)?;
		let range = self.write_range_at(&meta_blob_range, 128)?;
		return Ok(range);
	}
	fn write_blocks(
		&mut self,
		container: &Box<dyn container::Reader>,
	) -> std::io::Result<ByteRange> {
		let level_min = container.get_minimum_zoom();
		let level_max = container.get_maximum_zoom();

		let mut todos: Vec<BlockDefinition> = Vec::new();

		for level in level_min..=level_max {
			let level_row_min = container.get_minimum_row(level) as i64;
			let level_row_max = container.get_maximum_row(level) as i64;
			let level_col_min = container.get_minimum_col(level) as i64;
			let level_col_max = container.get_maximum_col(level) as i64;
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
		let sum = todos.iter().map(|x| x.count).sum();

		let bar = ProgressBar::new(sum);
		bar.set_style(
			ProgressStyle::with_template(
				"{wide_bar:0.white/dim.white} {pos:>7}/{len:7} {per_sec:12} {eta_precise}",
			)
			.unwrap()
			.progress_chars("██▁"),
		);

		let mut index = BlockIndex::new();

		for todo in todos {
			let range = self.write_block(&todo, &container, &bar)?;
			if range.length > 0 {
				continue;
			}
			index.add(&todo.level, &todo.block_row, &todo.block_col, &range)?;
		}

		bar.finish();

		let range = self.write_vec_brotli(&index.as_vec())?;
		return Ok(range);
	}
	fn write_block(
		&mut self,
		block: &BlockDefinition,
		reader: &Box<dyn container::Reader>,
		bar: &ProgressBar,
	) -> std::io::Result<ByteRange> {
		let mut index = TileIndex::new(block.row_min, block.row_max, block.col_min, block.col_max)?;

		let hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let mut tiles: Vec<(u64, u64)> = Vec::new();
		for row_in_block in block.row_min..=block.row_max {
			for col_in_block in block.col_min..=block.col_max {
				tiles.push((row_in_block, col_in_block))
			}
		}

		let wrapped_reader = ReaderWrapper::new(reader);

		let reader_mutex = Mutex::new(wrapped_reader);
		let writer_mutex = Mutex::new(&mut self.file_buffer);
		let index_mutex = Mutex::new(&mut index);
		let hash_lookup_mutex = Mutex::new(hash_lookup);

		let get_tile = |level, col, row| -> Vec<u8> {
			let save_container = reader_mutex.lock().unwrap();
			if self.tile_recompress {
				return save_container.get_tile_uncompressed(level, col, row);
			} else {
				return save_container.get_tile_raw(level, col, row);
			}
		};

		tiles.par_iter().for_each(|todo| {
			let row_in_block = todo.0;
			let col_in_block = todo.1;
			bar.inc(1);

			let row = block.block_row * 256 + row_in_block;
			let col = block.block_col * 256 + col_in_block;

			let tile = get_tile(block.level, col, row);

			let mut store_duplicate: bool = false;
			if tile.len() < 1000 {
				let hash_lookup = hash_lookup_mutex.lock().unwrap();
				if hash_lookup.contains_key(&tile) {
					index_mutex
						.lock()
						.unwrap()
						.add(hash_lookup.get(&tile).unwrap())
						.unwrap();
					return;
				} else {
					store_duplicate = true;
				}
				drop(hash_lookup);
			}

			let compressed;
			let temp;
			if self.tile_recompress {
				compressed = match self.tile_compression {
					Some(TileCompression::None) => &tile,
					Some(TileCompression::Gzip) => {
						temp = compress_gzip(&tile);
						&temp
					}
					Some(TileCompression::Brotli) => {
						temp = compress_brotli(&tile);
						&temp
					}
					None => panic!(),
				};
			} else {
				compressed = &tile;
			}
			let mut writer = writer_mutex.lock().unwrap();
			let range = ByteRange::new(
				writer.stream_position().unwrap(),
				writer.write(compressed).unwrap() as u64,
			);
			drop(writer);

			index_mutex.lock().unwrap().add(&range).unwrap();

			if store_duplicate {
				let mut hash_lookup = hash_lookup_mutex.lock().unwrap();
				hash_lookup.insert(tile, range);
				drop(hash_lookup);
			}
		});

		let range = self.write_vec_brotli(&index.as_vec())?;
		return Ok(range);

		fn compress_gzip(data: &Vec<u8>) -> Vec<u8> {
			let mut buffer: Vec<u8> = Vec::new();
			GzEncoder::new(data.as_slice(), Compression::best())
				.read_to_end(&mut buffer)
				.unwrap();
			return buffer;
		}

		fn compress_brotli(data: &Vec<u8>) -> Vec<u8> {
			let mut params = BrotliEncoderParams::default();
			params.quality = 11;
			params.size_hint = data.len();
			let mut cursor = Cursor::new(data);
			let mut compressed: Vec<u8> = Vec::new();
			BrotliCompress(&mut cursor, &mut compressed, &params).unwrap();
			return compressed;
		}
	}
	fn write(&mut self, buf: &[u8]) -> std::io::Result<ByteRange> {
		return Ok(ByteRange::new(
			self.file_buffer.stream_position()?,
			self.file_buffer.write(buf)? as u64,
		));
	}
	/*
	fn write_vec(&mut self, data: &Vec<u8>) -> std::io::Result<ByteRange> {
		return Ok(ByteRange::new(
			self.file_buffer.stream_position()?,
			self.file_buffer.write(&data)? as u64,
		));
	}
	fn write_vec_gzip(&mut self, data: &Vec<u8>) -> std::io::Result<ByteRange> {
		let mut buffer: Vec<u8> = Vec::new();
		GzEncoder::new(data.as_slice(), Compression::best()).read_to_end(&mut buffer)?;
		return self.write_vec(&buffer);
	}
	*/
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

struct ByteRange {
	offset: u64,
	length: u64,
}
impl ByteRange {
	fn new(offset: u64, length: u64) -> ByteRange {
		return ByteRange { offset, length };
	}
	fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
		writer.write(&self.offset.to_le_bytes())?;
		writer.write(&self.length.to_le_bytes())?;
		return Ok(());
	}
}

struct BlockDefinition {
	level: u64,
	block_row: u64,
	block_col: u64,
	row_min: u64,
	row_max: u64,
	col_min: u64,
	col_max: u64,
	count: u64,
}

struct BlockIndex {
	cursor: Cursor<Vec<u8>>,
}

impl BlockIndex {
	fn new() -> BlockIndex {
		let data = Vec::new();
		let cursor = Cursor::new(data);
		return BlockIndex { cursor };
	}
	fn add(&mut self, level: &u64, row: &u64, col: &u64, range: &ByteRange) -> std::io::Result<()> {
		self.cursor.write(&level.to_le_bytes())?;
		self.cursor.write(&row.to_le_bytes())?;
		self.cursor.write(&col.to_le_bytes())?;
		self.cursor.write(&range.offset.to_le_bytes())?;
		self.cursor.write(&range.length.to_le_bytes())?;
		return Ok(());
	}
	fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}

struct TileIndex {
	cursor: Cursor<Vec<u8>>,
}

impl TileIndex {
	fn new(row_min: u64, row_max: u64, col_min: u64, col_max: u64) -> std::io::Result<TileIndex> {
		let data = Vec::new();
		let mut cursor = Cursor::new(data);
		cursor.write(&(row_min as u8).to_le_bytes())?;
		cursor.write(&(row_max as u8).to_le_bytes())?;
		cursor.write(&(col_min as u8).to_le_bytes())?;
		cursor.write(&(col_max as u8).to_le_bytes())?;
		return Ok(TileIndex { cursor });
	}
	fn add(&mut self, range: &ByteRange) -> std::io::Result<()> {
		self.cursor.write(&range.offset.to_le_bytes())?;
		self.cursor.write(&range.length.to_le_bytes())?;
		return Ok(());
	}
	fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}
