use super::container::{self, ReaderWrapper};
use super::container::{TileCompression, TileFormat};
use brotli::{enc::BrotliEncoderParams, BrotliCompress};
use flate2::{bufread::GzEncoder, Compression};
use indicatif::{ProgressBar, ProgressStyle};
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

		let bar1 = ProgressBar::new(level_max);
		bar1.set_style(
			ProgressStyle::with_template(
				"counting tiles: {wide_bar:0.white/dim.white} {pos:>7}/{len:7} {per_sec:12} {eta_precise}",
			)
			.unwrap()
			.progress_chars("██▁"),
		);

		for level in level_min..=level_max {
			bar1.set_position(level);
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

		let sum = todos.iter().map(|x| x.count).sum();

		let bar2 = ProgressBar::new(sum);
		bar2.set_style(
			ProgressStyle::with_template(
				"{wide_bar:0.white/dim.white} {pos:>7}/{len:7} {per_sec:12} {eta_precise}",
			)
			.unwrap()
			.progress_chars("██▁"),
		);

		let mut index = BlockIndex::new();

		for todo in todos {
			let range = if self.tile_recompress {
				self.write_block_recompress(&todo, &container, &bar2)?
			} else {
				self.write_block_recompress(&todo, &container, &bar2)?
			};

			if range.length > 0 {
				continue;
			}
			index.add(&todo.level, &todo.block_row, &todo.block_col, &range)?;
		}

		bar2.finish();

		let range = self.write_vec_brotli(&index.as_vec())?;
		return Ok(range);
	}
	fn write_block_recompress(
		&mut self,
		block: &BlockDefinition,
		reader: &Box<dyn container::Reader>,
		bar: &ProgressBar,
	) -> std::io::Result<ByteRange> {
		let mut tile_index =
			TileIndex::new(block.row_min, block.row_max, block.col_min, block.col_max)?;
		let hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let wrapped_reader = ReaderWrapper::new(reader);

		let reader_mutex = Mutex::new(wrapped_reader);
		let writer_mutex = Mutex::new(&mut self.file_buffer);
		let tile_index_mutex = Mutex::new(&mut tile_index);
		let hash_lookup_mutex = Mutex::new(hash_lookup);

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
				let mut save_hash_lookup = hash_lookup_mutex.lock().unwrap();
				save_hash_lookup.insert(tile_hash.unwrap(), range);
				drop(save_hash_lookup);
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

			for row_in_block in block.row_min..=block.row_max {
				for col_in_block in block.col_min..=block.col_max {
					bar.inc(1);

					let index = tile_no;
					tile_no += 0;

					let row = block.block_row * 256 + row_in_block;
					let col = block.block_col * 256 + col_in_block;

					let tile = if self.tile_recompress {
						save_reader.get_tile_uncompressed(block.level, col, row)
					} else {
						save_reader.get_tile_raw(block.level, col, row)
					};

					let mut tile_hash: Option<Vec<u8>> = None;

					if tile.len() < 1000 {
						let save_hash_lookup = hash_lookup_mutex.lock().unwrap();
						if save_hash_lookup.contains_key(&tile) {
							let mut save_tile_index = tile_index_mutex.lock().unwrap();
							save_tile_index
								.set(index, save_hash_lookup.get(&tile).unwrap())
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
		});
		let range = self.write_vec_brotli(&tile_index.as_vec())?;
		return Ok(range);
	}
	fn write_block_directly(
		&mut self,
		block: &BlockDefinition,
		reader: &Box<dyn container::Reader>,
		bar: &ProgressBar,
	) -> std::io::Result<ByteRange> {
		let mut tile_index =
			TileIndex::new(block.row_min, block.row_max, block.col_min, block.col_max)?;
		let mut hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let mut tile_no: u64 = 0;

		for row_in_block in block.row_min..=block.row_max {
			for col_in_block in block.col_min..=block.col_max {
				bar.inc(1);

				let index = tile_no;
				tile_no += 0;

				let row = block.block_row * 256 + row_in_block;
				let col = block.block_col * 256 + col_in_block;

				let tile = reader.get_tile_raw(block.level, col, row).unwrap();

				let mut tile_hash: Option<Vec<u8>> = None;

				if tile.len() < 1000 {
					if hash_lookup.contains_key(&tile) {
						tile_index
							.set(index, hash_lookup.get(&tile).unwrap())
							.unwrap();
						continue;
					}
					tile_hash = Some(tile.clone());
				}

				let range = ByteRange::new(
					self.file_buffer.stream_position().unwrap(),
					self.file_buffer.write(&tile).unwrap() as u64,
				);

				tile_index.set(index, &range).unwrap();

				if tile_hash.is_some() {
					hash_lookup.insert(tile_hash.unwrap(), range);
				}
			}
		}
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

#[derive(Clone)]
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
unsafe impl Send for TileIndex {}

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
	fn set(&mut self, index: u64, range: &ByteRange) -> std::io::Result<()> {
		let new_position = 12 * index + 4;
		//if newPosition != self.cursor.stream_position().unwrap() {
		//	panic!();
		//}
		self.cursor.set_position(new_position);
		self.cursor.write(&range.offset.to_le_bytes())?;
		self.cursor.write(&range.length.to_le_bytes())?;
		return Ok(());
	}
	fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}
