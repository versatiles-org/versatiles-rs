use super::ByteRange;
use crate::opencloudtiles::{
	helpers::{compress_brotli, decompress_brotli},
	types::Blob,
};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{io::Cursor, ops::Div};

#[derive(Debug)]
pub struct TileIndex {
	index: Vec<ByteRange>,
}
unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn new_empty(count: usize) -> TileIndex {
		let mut index = Vec::new();
		index.resize(
			count,
			ByteRange {
				offset: 0,
				length: 0,
			},
		);
		return TileIndex { index };
	}
	pub fn from_blob(buf: Blob) -> TileIndex {
		let count = buf.len().div(12);
		assert_eq!(
			count * 12,
			buf.len(),
			"tile index is defect, cause buffer length is not a multiple of 12"
		);

		let mut index: Vec<ByteRange> = Vec::new();
		index.resize(count, ByteRange::new(0, 0));

		let mut cursor = Cursor::new(buf.as_slice());
		for i in 0..count {
			let offset = cursor.read_u64::<BE>().unwrap();
			let length = cursor.read_u32::<BE>().unwrap() as u64;
			index[i] = ByteRange { offset, length };
		}

		return TileIndex { index };
	}
	pub fn from_brotli_blob(buf: Blob) -> TileIndex {
		return TileIndex::from_blob(decompress_brotli(buf));
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
		return Blob::from_vec(cursor.into_inner());
	}
	pub fn as_brotli_blob(&self) -> Blob {
		return compress_brotli(self.as_blob());
	}
	pub fn get_tile_range(&self, index: usize) -> &ByteRange {
		return &self.index[index];
	}
}
