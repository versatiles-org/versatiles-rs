#![allow(dead_code)]

use crate::{
	types::{Blob, ByteRange, ValueReader, ValueReaderBlob},
	utils::{compress_brotli, decompress_brotli, BlobWriter},
};
use anyhow::{ensure, Result};
use std::ops::Div;

const TILE_INDEX_LENGTH: u64 = 12;

#[derive(Debug, PartialEq, Eq)]
pub struct TileIndex {
	index: Vec<ByteRange>,
}

unsafe impl Send for TileIndex {}

impl TileIndex {
	pub fn new_empty(count: usize) -> Self {
		let index = vec![ByteRange::new(0, 0); count];
		Self { index }
	}

	pub fn from_blob(blob: Blob) -> Result<Self> {
		let count = blob.len().div(TILE_INDEX_LENGTH);
		ensure!(
			count * TILE_INDEX_LENGTH == blob.len(),
			"Tile index is defective: buffer length is not a multiple of {}",
			TILE_INDEX_LENGTH
		);

		let mut index = Vec::new();
		let mut reader = ValueReaderBlob::new_be(&blob);
		for _ in 0..count {
			index.push(ByteRange::new(reader.read_u64()?, reader.read_u32()? as u64));
		}

		Ok(Self { index })
	}

	pub fn from_brotli_blob(buf: Blob) -> Result<Self> {
		Self::from_blob(decompress_brotli(&buf)?)
	}

	pub fn set(&mut self, index: usize, tile_byte_range: ByteRange) {
		self.index[index] = tile_byte_range;
	}

	pub fn as_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_be();
		for range in &self.index {
			writer.write_u64(range.offset)?;
			writer.write_u32(range.length as u32)?;
		}

		Ok(writer.into_blob())
	}

	pub fn as_brotli_blob(&self) -> Result<Blob> {
		compress_brotli(&self.as_blob()?)
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
	fn conversion() -> Result<()> {
		let mut index1 = TileIndex::new_empty(100);
		for i in 0..100u64 {
			index1.set(i as usize, ByteRange::new(i * 1000, i * 2000));
		}
		let index2 = TileIndex::from_brotli_blob(index1.as_brotli_blob()?)?;
		assert_eq!(index1, index2);

		Ok(())
	}
}
