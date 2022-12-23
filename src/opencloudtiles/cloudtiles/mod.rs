use std::io::{Cursor, Write};

mod converter;
mod reader;

pub use converter::TileConverter;
pub use reader::TileReader;

#[derive(Clone)]
struct ByteRange {
	offset: u64,
	length: u64,
}
impl ByteRange {
	fn new(offset: u64, length: u64) -> ByteRange {
		return ByteRange { offset, length };
	}
	fn write_to(&self, writer: &mut impl Write) -> Result<(), &'static str> {
		writer.write(&self.offset.to_le_bytes()).unwrap();
		writer.write(&self.length.to_le_bytes()).unwrap();
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
	fn add(
		&mut self,
		level: &u64,
		row: &u64,
		col: &u64,
		range: &ByteRange,
	) -> Result<(), &'static str> {
		self.cursor.write(&level.to_le_bytes()).unwrap();
		self.cursor.write(&row.to_le_bytes()).unwrap();
		self.cursor.write(&col.to_le_bytes()).unwrap();
		self.cursor.write(&range.offset.to_le_bytes()).unwrap();
		self.cursor.write(&range.length.to_le_bytes()).unwrap();
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
	fn new(
		row_min: u64,
		row_max: u64,
		col_min: u64,
		col_max: u64,
	) -> Result<TileIndex, &'static str> {
		let data = Vec::new();
		let mut cursor = Cursor::new(data);
		cursor.write(&(row_min as u8).to_le_bytes()).unwrap();
		cursor.write(&(row_max as u8).to_le_bytes()).unwrap();
		cursor.write(&(col_min as u8).to_le_bytes()).unwrap();
		cursor.write(&(col_max as u8).to_le_bytes()).unwrap();
		return Ok(TileIndex { cursor });
	}
	fn set(&mut self, index: u64, range: &ByteRange) -> Result<(), &'static str> {
		let new_position = 12 * index + 4;
		//if newPosition != self.cursor.stream_position().unwrap() {
		//	panic!();
		//}
		self.cursor.set_position(new_position);
		self.cursor.write(&range.offset.to_le_bytes()).unwrap();
		self.cursor.write(&range.length.to_le_bytes()).unwrap();
		return Ok(());
	}
	fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}
