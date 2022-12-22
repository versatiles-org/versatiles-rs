use brotli::{enc::BrotliEncoderParams, BrotliCompress};
use flate2::{bufread::GzEncoder, Compression};
use std::io::{Cursor, Read, Write};

mod converter;
mod reader;

pub use converter::Converter;
pub use reader::Reader;

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
