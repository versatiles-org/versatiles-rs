use super::ByteRange;
use crate::shared::{compress_brotli, decompress_brotli, Blob};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{io::Cursor, ops::Div};

const TILE_INDEX_LENGTH: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub struct TileIndex {
	index: Vec<ByteRange>,
}
unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn new_empty(count: usize) -> TileIndex {
		let mut index = Vec::new();
		index.resize(count, ByteRange { offset: 0, length: 0 });

		TileIndex { index }
	}
	pub fn from_blob(buf: Blob) -> TileIndex {
		let count = buf.len().div(TILE_INDEX_LENGTH);
		assert_eq!(
			count * TILE_INDEX_LENGTH,
			buf.len(),
			"tile index is defect, cause buffer length is not a multiple of {}",
			TILE_INDEX_LENGTH
		);

		let mut index: Vec<ByteRange> = Vec::new();
		index.resize(count, ByteRange::new(0, 0));

		let mut cursor = Cursor::new(buf.as_slice());
		for item in index.iter_mut() {
			item.offset = cursor.read_u64::<BE>().unwrap();
			item.length = cursor.read_u32::<BE>().unwrap() as u64;
		}

		TileIndex { index }
	}
	pub fn from_brotli_blob(buf: Blob) -> TileIndex {
		TileIndex::from_blob(decompress_brotli(buf).unwrap())
	}
	pub fn set(&mut self, index: usize, tile_byte_range: ByteRange) {
		self.index[index] = tile_byte_range;
	}
	pub fn as_blob(&self) -> Blob {
		let buf = Vec::new();
		let mut cursor = Cursor::new(buf);
		for range in self.index.iter() {
			cursor.write_u64::<BE>(range.offset).unwrap();
			cursor.write_u32::<BE>(range.length as u32).unwrap();
		}

		Blob::from(cursor.into_inner())
	}
	pub fn as_brotli_blob(&self) -> Blob {
		compress_brotli(self.as_blob()).unwrap()
	}
	pub fn get(&self, index: usize) -> &ByteRange {
		&self.index[index]
	}
	pub fn len(&self) -> usize {
		self.index.len()
	}
	pub fn iter(&self) -> impl Iterator<Item = &ByteRange> {
		self.index.iter()
	}
	pub fn add_offset(&mut self, offset: u64) {
		self.index.iter_mut().for_each(|r| r.offset += offset);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn init() {
		const COUNT: u64 = 16;

		let mut index = TileIndex::new_empty(COUNT as usize);
		assert_eq!(index.len(), COUNT as usize);

		for i in 0..COUNT {
			index.set(i as usize, ByteRange::new(i * i, i));
			assert_eq!(index.get(i as usize), &ByteRange::new(i * i, i));
		}

		index.add_offset(18);

		for (index, range) in index.iter().enumerate() {
			let i = index as u64;
			assert_eq!(range, &ByteRange::new(i * i + 18, i));
		}
	}

	#[test]
	fn conversion() {
		let mut index1 = TileIndex::new_empty(100);
		for i in 0..100u64 {
			index1.set(i as usize, ByteRange::new(i * 1000, i * 2000));
		}
		let index2 = TileIndex::from_brotli_blob(index1.as_brotli_blob());
		assert_eq!(index1, index2);
	}
}
