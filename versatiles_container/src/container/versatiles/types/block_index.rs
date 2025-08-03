#![allow(dead_code)]

//! This module defines the `BlockIndex` struct, which represents an index of blocks within a tile set.
//!
//! The `BlockIndex` struct contains metadata about the blocks, including their coordinates and bounding boxes, and provides methods to manipulate and query this data.

use super::BlockDefinition;
use anyhow::{Result, ensure};
use std::{collections::HashMap, ops::Div};
use versatiles_core::{io::*, utils::*, *};
use versatiles_derive::context;

const BLOCK_INDEX_LENGTH: u64 = 33;

/// A struct representing an index of blocks within a tile set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockIndex {
	lookup: HashMap<TileCoord3, BlockDefinition>,
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
	pub fn from_blob(buf: Blob) -> Result<Self> {
		let count = buf.len().div(BLOCK_INDEX_LENGTH);
		ensure!(
			count * BLOCK_INDEX_LENGTH == buf.len(),
			"Block index is defective, because buffer length is not a multiple of {}",
			BLOCK_INDEX_LENGTH
		);

		let mut block_index = Self::new_empty();
		for i in 0..count {
			let range = &ByteRange::new(i * BLOCK_INDEX_LENGTH, BLOCK_INDEX_LENGTH);
			block_index.add_block(BlockDefinition::from_blob(&buf.read_range(range)?)?);
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
	pub fn from_brotli_blob(buf: Blob) -> Result<Self> {
		Self::from_blob(decompress_brotli(&buf)?)
	}

	/// Returns a `TileBBoxPyramid` representing the bounding boxes of the blocks in the index.
	///
	/// # Returns
	/// A `TileBBoxPyramid` representing the bounding boxes of the blocks.
	pub fn get_bbox_pyramid(&self) -> TileBBoxPyramid {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for (_coord, block) in self.lookup.iter() {
			pyramid.include_bbox(block.get_global_bbox());
		}

		pyramid
	}

	/// Adds a block to the index.
	///
	/// # Arguments
	/// * `block` - The block to add.
	pub fn add_block(&mut self, block: BlockDefinition) {
		self.lookup.insert(*block.get_coord3(), block);
	}

	/// Converts the `BlockIndex` to a binary blob.
	///
	/// # Returns
	/// A binary blob representing the `BlockIndex`.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	#[context("Failed to create BlockIndex from blob")]
	pub fn as_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_be();
		for (_coord, block) in self.lookup.iter() {
			writer.write_blob(&block.as_blob()?)?;
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
	pub fn as_brotli_blob(&self) -> Result<Blob> {
		compress_brotli_fast(&self.as_blob()?)
	}

	/// Retrieves a block from the index by its coordinates.
	///
	/// # Arguments
	/// * `coord` - The coordinates of the block.
	///
	/// # Returns
	/// An option containing a reference to the block if found, or `None` if not found.
	pub fn get_block(&self, coord: &TileCoord3) -> Option<&BlockDefinition> {
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
	use super::*;
	use versatiles_core::TileBBox;

	#[test]
	fn conversion() -> Result<()> {
		let mut index1 = BlockIndex::new_empty();
		index1.add_block(BlockDefinition::new(&TileBBox::new(3, 1, 2, 3, 4)?));
		let index2 = BlockIndex::from_brotli_blob(index1.as_brotli_blob()?)?;
		assert_eq!(index1, index2);
		Ok(())
	}
}
