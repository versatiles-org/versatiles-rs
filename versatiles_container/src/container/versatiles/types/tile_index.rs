#![allow(dead_code)]

//! This module defines the `TileIndex` struct, which represents an index of tile byte ranges.
//!
//! The `TileIndex` struct is used to manage the byte ranges of tiles within a versatiles file. It provides methods to create, manipulate, and convert the index to and from binary blobs.

use anyhow::{Result, ensure};
use std::ops::Div;
use versatiles_core::{
	Blob, ByteRange,
	compression::{compress_brotli_fast, decompress_brotli},
	io::{ValueReader, ValueReaderBlob, ValueWriter, ValueWriterBlob},
};
use versatiles_derive::context;

const TILE_INDEX_LENGTH: u64 = 12;

/// A struct representing an index of tile byte ranges.
#[derive(Debug, PartialEq, Eq)]
pub struct TileIndex {
	index: Vec<ByteRange>,
}

unsafe impl Send for TileIndex {}

impl TileIndex {
	/// Creates a new empty `TileIndex` with a specified count.
	///
	/// # Arguments
	/// * `count` - The number of byte ranges in the index.
	pub fn new_empty(count: usize) -> Self {
		let index = vec![ByteRange::new(0, 0); count];
		Self { index }
	}

	/// Creates a `TileIndex` from a binary blob.
	///
	/// # Arguments
	/// * `blob` - The binary data representing the tile index.
	///
	/// # Errors
	/// Returns an error if the binary data cannot be parsed correctly.
	#[context("Failed to create TileIndex from blob")]
	pub fn from_blob(blob: Blob) -> Result<Self> {
		let count = blob.len().div(TILE_INDEX_LENGTH);
		ensure!(
			count * TILE_INDEX_LENGTH == blob.len(),
			"Tile index is defective: buffer length is not a multiple of {TILE_INDEX_LENGTH}"
		);

		let mut index = Vec::new();
		let mut reader = ValueReaderBlob::new_be(blob);
		for _ in 0..count {
			index.push(ByteRange::new(reader.read_u64()?, u64::from(reader.read_u32()?)));
		}

		Ok(Self { index })
	}

	/// Creates a `TileIndex` from a Brotli compressed binary blob.
	///
	/// # Arguments
	/// * `buf` - The compressed binary data representing the tile index.
	///
	/// # Errors
	/// Returns an error if the compressed binary data cannot be decompressed or parsed correctly.
	#[context("Failed to create TileIndex from Brotli blob")]
	pub fn from_brotli_blob(buf: Blob) -> Result<Self> {
		Self::from_blob(decompress_brotli(&buf)?)
	}

	/// Sets the byte range for a specific index.
	///
	/// # Arguments
	/// * `index` - The index to set the byte range for.
	/// * `tile_byte_range` - The byte range to set.
	pub fn set(&mut self, index: usize, tile_byte_range: ByteRange) {
		self.index[index] = tile_byte_range;
	}

	/// Converts the `TileIndex` to a binary blob.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	#[context("Failed to create TileIndex from blob")]
	pub fn as_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_be();
		for range in &self.index {
			writer.write_u64(range.offset)?;
			writer.write_u32(u32::try_from(range.length)?)?;
		}

		Ok(writer.into_blob())
	}

	/// Converts the `TileIndex` to a Brotli compressed binary blob.
	///
	/// # Errors
	/// Returns an error if the compression or conversion fails.
	#[context("Failed to create TileIndex from Brotli blob")]
	pub fn as_brotli_blob(&self) -> Result<Blob> {
		compress_brotli_fast(&self.as_blob()?)
	}

	/// Gets the byte range for a specific index.
	///
	/// # Arguments
	/// * `index` - The index to get the byte range for.
	///
	/// # Returns
	/// The byte range at the specified index.
	pub fn get(&self, index: usize) -> &ByteRange {
		&self.index[index]
	}

	/// Returns the number of byte ranges in the index.
	pub fn len(&self) -> usize {
		self.index.len()
	}

	/// Returns an iterator over the byte ranges in the index.
	pub fn iter(&self) -> impl Iterator<Item = &ByteRange> {
		self.index.iter()
	}

	/// Adds an offset to all byte ranges in the index.
	///
	/// # Arguments
	/// * `offset` - The offset to add to each byte range.
	pub fn add_offset(&mut self, offset: u64) {
		self.index.iter_mut().for_each(|r| r.offset += offset);
	}
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
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
