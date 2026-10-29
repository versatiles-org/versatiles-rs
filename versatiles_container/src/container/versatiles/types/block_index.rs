//! This module defines the `BlockIndex` struct, which represents an index of blocks within a tile set.
//!
//! The `BlockIndex` struct contains metadata about the blocks, including their coordinates and bounding boxes, and provides methods to manipulate and query this data.

use std::collections::HashMap;

use anyhow::{Result, ensure};
use versatiles_core::{
	Blob, TileCoord, TilePyramid,
	compression::{compress_brotli_fast, decompress_brotli},
	io::{ValueWriter, ValueWriterBlob},
};
use versatiles_derive::context;

use super::BlockDefinition;

const BLOCK_INDEX_LENGTH: u64 = 33;

/// A struct representing an index of blocks within a tile set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockIndex {
	lookup: HashMap<TileCoord, BlockDefinition>,
}

impl BlockIndex {
	/// Creates a new empty `BlockIndex`.
	///
	/// # Returns
	/// A new empty `BlockIndex`.
	pub fn new_empty() -> Self {
		Self { lookup: HashMap::new() }
	}

	/// Creates a `BlockIndex` from a binary blob.
	///
	/// # Arguments
	/// * `buf` - The binary data representing the block index.
	///
	/// # Errors
	/// Returns an error if the binary data cannot be parsed correctly.
	#[context("Failed to create BlockIndex from blob")]
	pub fn from_blob(buf: &Blob) -> Result<Self> {
		ensure!(
			buf.len().is_multiple_of(BLOCK_INDEX_LENGTH),
			"Block index is defective, because buffer length is not a multiple of {BLOCK_INDEX_LENGTH}"
		);

		let mut block_index = Self::new_empty();
		for chunk in buf.as_slice().chunks_exact(usize::try_from(BLOCK_INDEX_LENGTH)?) {
			let block = BlockDefinition::from_blob(&Blob::from(chunk))?;
			// Two entries for one block would leave it to the order of the index
			// which of them is read, so the spec has readers reject them.
			let coord = *block.coord();
			ensure!(
				block_index.lookup.insert(coord, block).is_none(),
				"Block index is defective, because it lists the block at level {}, column {}, row {} more than once",
				coord.level,
				coord.x,
				coord.y
			);
		}

		Ok(block_index)
	}

	/// Creates a `BlockIndex` from a Brotli compressed binary blob.
	///
	/// # Arguments
	/// * `buf` - The Brotli compressed binary data representing the block index.
	///
	/// # Errors
	/// Returns an error if the binary data cannot be decompressed or parsed correctly.
	#[context("Failed to create BlockIndex from Brotli blob")]
	pub fn from_brotli_blob(buf: &Blob) -> Result<Self> {
		Self::from_blob(&decompress_brotli(buf)?)
	}

	/// Returns a [`TilePyramid`] representing the bounding boxes of the blocks in the index.
	pub fn tile_pyramid(&self) -> TilePyramid {
		let mut pyramid = TilePyramid::new_empty();
		for block in self.lookup.values() {
			pyramid.insert_bbox(block.global_bbox()).expect("include_bbox failed");
		}
		pyramid
	}

	/// Adds a block to the index.
	///
	/// # Arguments
	/// * `block` - The block to add.
	pub fn insert_block(&mut self, block: BlockDefinition) {
		self.lookup.insert(*block.coord(), block);
	}

	/// Converts the `BlockIndex` to a binary blob.
	///
	/// # Returns
	/// A binary blob representing the `BlockIndex`.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	#[context("Failed to create BlockIndex from blob")]
	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_be();
		for block in self.lookup.values() {
			writer.write_blob(&block.to_blob()?)?;
		}

		Ok(writer.into_blob())
	}

	/// Converts the `BlockIndex` to a Brotli compressed binary blob.
	///
	/// # Returns
	/// A Brotli compressed binary blob representing the `BlockIndex`.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	#[context("Failed to create BlockIndex from Brotli blob")]
	pub fn to_brotli_blob(&self) -> Result<Blob> {
		compress_brotli_fast(&self.to_blob()?)
	}

	/// Retrieves a block from the index by its coordinates.
	///
	/// # Arguments
	/// * `coord` - The coordinates of the block.
	///
	/// # Returns
	/// An option containing a reference to the block if found, or `None` if not found.
	pub fn block(&self, coord: &TileCoord) -> Option<&BlockDefinition> {
		self.lookup.get(coord)
	}

	/// Returns the number of blocks in the index.
	///
	/// # Returns
	/// The number of blocks in the index.
	pub fn len(&self) -> usize {
		self.lookup.len()
	}

	/// Returns an iterator over the blocks in the index.
	///
	/// # Returns
	/// An iterator over the blocks in the index.
	pub fn iter(&self) -> impl Iterator<Item = &BlockDefinition> {
		self.lookup.values()
	}
}

#[cfg(test)]
mod tests {
	use versatiles_core::TileBBox;

	use super::*;

	#[test]
	fn conversion() -> Result<()> {
		let mut index1 = BlockIndex::new_empty();
		index1.insert_block(BlockDefinition::new(&TileBBox::from_min_and_max(3, 1, 2, 3, 4)?)?);
		let blob = index1.to_brotli_blob()?;
		let index2 = BlockIndex::from_brotli_blob(&blob)?;
		assert_eq!(index1, index2);
		Ok(())
	}

	#[test]
	fn several_blocks_are_read_one_record_each() -> Result<()> {
		let mut index1 = BlockIndex::new_empty();
		for (level, x, y) in [(4, 1, 2), (9, 300, 400), (14, 1000, 2000)] {
			index1.insert_block(BlockDefinition::new(&TileBBox::from_min_and_max(
				level,
				x,
				y,
				x + 5,
				y + 7,
			)?)?);
		}
		let index2 = BlockIndex::from_blob(&index1.to_blob()?)?;
		assert_eq!(index2.len(), 3);
		assert_eq!(index1, index2);
		Ok(())
	}

	#[test]
	fn a_block_listed_twice_is_rejected() -> Result<()> {
		let mut index = BlockIndex::new_empty();
		index.insert_block(BlockDefinition::new(&TileBBox::from_min_and_max(
			9, 300, 400, 305, 407,
		)?)?);
		let entry = index.to_blob()?;
		let twice = Blob::from([entry.as_slice(), entry.as_slice()].concat());
		let error = format!("{:#}", BlockIndex::from_blob(&twice).unwrap_err());
		assert!(error.contains("more than once"), "{error}");
		assert!(error.contains("level 9, column 1, row 1"), "{error}");
		Ok(())
	}

	#[test]
	fn a_truncated_block_index_is_rejected() -> Result<()> {
		let mut index = BlockIndex::new_empty();
		index.insert_block(BlockDefinition::new(&TileBBox::from_min_and_max(3, 1, 2, 3, 4)?)?);
		let blob = index.to_blob()?;
		let truncated = Blob::from(&blob.as_slice()[..32]);
		let error = format!("{:#}", BlockIndex::from_blob(&truncated).unwrap_err());
		assert!(error.contains("not a multiple of 33"), "{error}");
		Ok(())
	}
}
