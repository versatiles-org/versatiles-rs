use std::io::{Cursor, Write};

#[derive(Clone)]
pub struct ByteRange {
	pub offset: u64,
	pub length: u64,
}
impl ByteRange {
	pub fn new(offset: u64, length: u64) -> ByteRange {
		return ByteRange { offset, length };
	}
	pub fn write_to(&self, writer: &mut impl Write) {
		writer.write(&self.offset.to_le_bytes()).unwrap();
		writer.write(&self.length.to_le_bytes()).unwrap();
	}
}

pub struct BlockDefinition {
	pub level: u64,
	pub block_row: u64,
	pub block_col: u64,
	pub row_min: u64,
	pub row_max: u64,
	pub col_min: u64,
	pub col_max: u64,
	pub count: u64,
}

pub struct BlockIndex {
	cursor: Cursor<Vec<u8>>,
}

impl BlockIndex {
	pub fn new() -> BlockIndex {
		let data = Vec::new();
		let cursor = Cursor::new(data);
		return BlockIndex { cursor };
	}
	pub fn add(
		&mut self,
		level: &u64,
		row: &u64,
		col: &u64,
		range: &ByteRange,
	) {
		self.cursor.write(&level.to_le_bytes()).unwrap();
		self.cursor.write(&row.to_le_bytes()).unwrap();
		self.cursor.write(&col.to_le_bytes()).unwrap();
		self.cursor.write(&range.offset.to_le_bytes()).unwrap();
		self.cursor.write(&range.length.to_le_bytes()).unwrap();
	}
	pub fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}

pub struct TileIndex {
	cursor: Cursor<Vec<u8>>,
}
unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn new(row_min: u64, row_max: u64, col_min: u64, col_max: u64) -> TileIndex {
		let data = Vec::new();
		let mut cursor = Cursor::new(data);
		cursor.write(&(row_min as u8).to_le_bytes()).unwrap();
		cursor.write(&(row_max as u8).to_le_bytes()).unwrap();
		cursor.write(&(col_min as u8).to_le_bytes()).unwrap();
		cursor.write(&(col_max as u8).to_le_bytes()).unwrap();
		return TileIndex { cursor };
	}
	pub fn set(&mut self, index: u64, range: &ByteRange) {
		let new_position = 12 * index + 4;
		//if newPosition != self.cursor.stream_position().unwrap() {
		//	panic!();
		//}
		self.cursor.set_position(new_position);
		self.cursor.write(&range.offset.to_le_bytes()).unwrap();
		self.cursor.write(&range.length.to_le_bytes()).unwrap();
	}
	pub fn as_vec(&self) -> &Vec<u8> {
		return self.cursor.get_ref();
	}
}
