//! This module defines the `TileIndex` struct, which represents an index of tile byte ranges.
//!
//! The `TileIndex` struct is used to manage the byte ranges of tiles within a versatiles file. It provides methods to create, manipulate, and convert the index to and from binary blobs.

use std::ops::Div;

use anyhow::{Context, Result, ensure};
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

impl TileIndex {
	/// Creates a new `TileIndex` with a specified count.
	///
	/// # Arguments
	/// * `count` - The number of byte ranges in the index.
	pub fn new(count: usize) -> Self {
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

		let mut index = Vec::with_capacity(usize::try_from(count)?);
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
	pub fn from_brotli_blob(buf: &Blob) -> Result<Self> {
		Self::from_blob(decompress_brotli(buf)?)
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
	pub fn to_blob(&self) -> Result<Blob> {
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
	pub fn to_brotli_blob(&self) -> Result<Blob> {
		compress_brotli_fast(&self.to_blob()?)
	}

	/// Gets the byte range for a specific index.
	///
	/// # Arguments
	/// * `index` - The index to get the byte range for.
	///
	/// # Returns
	/// The byte range at the specified index.
	///
	/// # Errors
	/// Returns an error if `index` is out of bounds.
	///
	/// Fallible because the two numbers involved come from different parts of an
	/// untrusted file: the caller's slot number is derived from a block
	/// definition in the block index, while this index's length comes from a
	/// separate Brotli-compressed blob. A crafted container can make them
	/// disagree, so an unchecked `self.index[index]` is a panic a file can ask
	/// for. `from_blob` is the other half of the guarantee — it refuses a blob
	/// whose length is not a whole number of entries.
	pub fn get(&self, index: usize) -> Result<&ByteRange> {
		self.index.get(index).with_context(|| {
			format!(
				"tile index holds {} entries, but entry {index} was requested",
				self.index.len()
			)
		})
	}

	/// Returns the number of byte ranges in the index.
	pub fn len(&self) -> usize {
		self.index.len()
	}

	/// Returns an iterator over the byte ranges in the index.
	pub fn iter(&self) -> impl Iterator<Item = &ByteRange> {
		self.index.iter()
	}

	/// Shifts all byte range offsets by a delta.
	///
	/// # Arguments
	/// * `delta` - The value to add to each byte range offset.
	///
	/// # Errors
	/// Returns an error if any resulting offset overflows `u64`.
	///
	/// Both operands come out of the file: the per-entry offsets from the
	/// decompressed index, `delta` from the block's tiles range. Unchecked, the
	/// sum wraps in a release build, and a wrapped offset either fails the read
	/// bounds test or — worse — lands somewhere valid.
	pub fn shift_by(&mut self, delta: u64) -> Result<()> {
		for range in &mut self.index {
			range.offset = range
				.offset
				.checked_add(delta)
				.with_context(|| format!("tile index offset {} + {delta} overflows u64", range.offset))?;
		}
		Ok(())
	}
}

#[cfg(test)]
#[expect(
	clippy::cast_possible_truncation,
	reason = "test data is built from literal counters"
)]
mod tests {
	use super::*;

	#[test]
	fn init() {
		const COUNT: u64 = 16;

		let mut index = TileIndex::new(COUNT as usize);
		assert_eq!(index.len(), COUNT as usize);

		for i in 0..COUNT {
			index.set(i as usize, ByteRange::new(i * i, i));
			assert_eq!(index.get(i as usize).unwrap(), &ByteRange::new(i * i, i));
		}

		index.shift_by(18).unwrap();

		for (index, range) in index.iter().enumerate() {
			let i = index as u64;
			assert_eq!(range, &ByteRange::new(i * i + 18, i));
		}
	}

	/// Both the per-entry offsets and the shift come out of the file, and release
	/// builds do not check overflow — so a sum that wraps would point a read at
	/// an unrelated part of the file instead of failing.
	#[test]
	fn a_shift_that_overflows_is_an_error() {
		let mut index = TileIndex::new(2);
		index.set(0, ByteRange::new(10, 5));
		index.set(1, ByteRange::new(u64::MAX - 1, 5));

		let error = index.shift_by(10).expect_err("an overflowing shift must be an error");
		assert!(format!("{error:#}").contains("overflows u64"));

		// A shift that fits still applies to every entry.
		let mut index = TileIndex::new(2);
		index.set(0, ByteRange::new(10, 5));
		index.set(1, ByteRange::new(20, 5));
		index.shift_by(7).unwrap();
		assert_eq!(index.get(0).unwrap().offset, 17);
		assert_eq!(index.get(1).unwrap().offset, 27);
	}

	/// The slot number and the index length come from different parts of an
	/// untrusted file, so a lookup past the end must be an error rather than a
	/// panic.
	#[test]
	fn out_of_bounds_lookup_is_an_error() {
		let index = TileIndex::new(4);

		assert!(index.get(3).is_ok());
		assert!(index.get(4).is_err());
		assert!(index.get(usize::MAX).is_err());

		let message = index.get(1285).unwrap_err().to_string();
		assert!(
			message.contains("holds 4 entries") && message.contains("entry 1285"),
			"unhelpful message: {message}"
		);
	}

	#[test]
	fn conversion() -> Result<()> {
		let mut index1 = TileIndex::new(100);
		for i in 0..100u64 {
			index1.set(i as usize, ByteRange::new(i * 1000, i * 2000));
		}
		let blob = index1.to_brotli_blob()?;
		let index2 = TileIndex::from_brotli_blob(&blob)?;
		assert_eq!(index1, index2);

		Ok(())
	}
}
