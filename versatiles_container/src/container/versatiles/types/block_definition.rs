#![allow(dead_code)]

//! This module defines the `BlockDefinition` struct which represents a block of tiles within a larger tile set.
//!
//! The `BlockDefinition` struct contains metadata about the tile block, including its coordinates, bounding box, and byte ranges for tiles and index data.

use anyhow::{Result, ensure};
use std::{fmt, ops::Div};
use versatiles_core::{io::*, *};
use versatiles_derive::context;

/// A struct representing a block of tiles within a larger tile set.
#[derive(Clone, PartialEq, Eq)]
pub struct BlockDefinition {
	offset: TileCoord,        // block offset, for level 14 it's between [0,0] and [63,63]
	global_bbox: TileBBox,    // tile coverage, is usually [0,0,255,255]
	tiles_coverage: TileBBox, // tile coverage, is usually [0,0,255,255]
	tiles_range: ByteRange,
	index_range: ByteRange,
}

impl BlockDefinition {
	/// Creates a new `BlockDefinition` from a given bounding box.
	///
	/// # Arguments
	/// * `bbox` - The bounding box of the tiles.
	///
	/// # Returns
	/// A new `BlockDefinition` instance.
	pub fn new(bbox: &TileBBox) -> Result<Self> {
		ensure!(!bbox.is_empty(), "bbox must not be empty");
		ensure!(bbox.width() <= 256, "bbox width must be <= 256");
		ensure!(bbox.height() <= 256, "bbox height must be <= 256");

		let x_min = bbox.x_min().div(256);
		let y_min = bbox.y_min().div(256);
		let level = bbox.level;
		let global_bbox: TileBBox = *bbox;

		let tiles_coverage = TileBBox::from_min_wh(
			level.min(8),
			bbox.x_min() - x_min * 256,
			bbox.y_min() - y_min * 256,
			bbox.width(),
			bbox.height(),
		)?;

		Ok(Self {
			offset: TileCoord::new(level, x_min, y_min).unwrap(),
			global_bbox,
			tiles_coverage,
			tiles_range: ByteRange::empty(),
			index_range: ByteRange::empty(),
		})
	}

	/// Creates a `BlockDefinition` from a binary blob.
	///
	/// # Arguments
	/// * `blob` - The binary data representing the block definition.
	///
	/// # Errors
	/// Returns an error if the binary data cannot be parsed correctly.
	#[context("Failed to create BlockDefinition from blob")]
	pub fn from_blob(blob: &Blob) -> Result<Self> {
		let mut reader = ValueReaderSlice::new_be(blob.as_slice());

		let level = reader.read_u8()?;
		let x = reader.read_u32()?;
		let y = reader.read_u32()?;

		let x_min = reader.read_u8()? as u32;
		let y_min = reader.read_u8()? as u32;
		let x_max = reader.read_u8()? as u32;
		let y_max = reader.read_u8()? as u32;

		let tiles_bbox = TileBBox::from_boundaries(level.min(8), x_min, y_min, x_max, y_max)?;

		let offset = reader.read_u64()?;
		let tiles_length = reader.read_u64()?;
		let index_length = reader.read_u32()? as u64;

		let tiles_range = ByteRange::new(offset, tiles_length);
		let index_range = ByteRange::new(offset + tiles_length, index_length);

		let global_bbox = TileBBox::from_boundaries(
			level,
			x_min + x * 256,
			y_min + y * 256,
			x_max + x * 256,
			y_max + y * 256,
		)?;

		Ok(Self {
			offset: TileCoord::new(level, x, y)?,
			global_bbox,
			tiles_coverage: tiles_bbox,
			tiles_range,
			index_range,
		})
	}

	/// Sets the byte range for the tiles data.
	///
	/// # Arguments
	/// * `range` - The byte range for the tiles data.
	pub fn set_tiles_range(&mut self, range: ByteRange) {
		self.tiles_range = range;
	}

	/// Sets the byte range for the index data.
	///
	/// # Arguments
	/// * `range` - The byte range for the index data.
	pub fn set_index_range(&mut self, range: ByteRange) {
		self.index_range = range;
	}

	/// Returns the number of tiles in the block.
	///
	/// # Returns
	/// The number of tiles in the block.
	pub fn count_tiles(&self) -> u64 {
		self.tiles_coverage.count_tiles()
	}

	/// Converts the `BlockDefinition` to a binary blob.
	///
	/// # Returns
	/// A binary blob representing the `BlockDefinition`.
	///
	/// # Errors
	/// Returns an error if the conversion fails.
	#[context("Failed to create BlockDefinition from blob")]
	pub fn as_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_be();
		writer.write_u8(self.offset.level)?;
		writer.write_u32(self.offset.x)?;
		writer.write_u32(self.offset.y)?;

		writer.write_u8(self.tiles_coverage.x_min() as u8)?;
		writer.write_u8(self.tiles_coverage.y_min() as u8)?;
		writer.write_u8(self.tiles_coverage.x_max() as u8)?;
		writer.write_u8(self.tiles_coverage.y_max() as u8)?;

		ensure!(
			self.tiles_range.offset + self.tiles_range.length == self.index_range.offset,
			"tiles_range and index_range do not match"
		);

		writer.write_u64(self.tiles_range.offset)?;
		writer.write_u64(self.tiles_range.length)?;
		writer.write_u32(self.index_range.length as u32)?;

		Ok(writer.into_blob())
	}

	/// Returns the sort index for the block.
	///
	/// # Returns
	/// The sort index for the block.
	#[allow(dead_code)]
	pub fn get_sort_index(&self) -> u64 {
		self.offset.get_sort_index()
	}

	/// Returns the global bounding box of the defined tiles.
	///
	/// # Returns
	/// A reference to the global bounding box of the defined tiles.
	pub fn get_global_bbox(&self) -> &TileBBox {
		&self.global_bbox
	}

	/// Returns the byte range for the tiles data.
	///
	/// # Returns
	/// A reference to the byte range for the tiles data.
	pub fn get_tiles_range(&self) -> &ByteRange {
		&self.tiles_range
	}

	/// Returns the byte range for the index data.
	///
	/// # Returns
	/// A reference to the byte range for the index data.
	pub fn get_index_range(&self) -> &ByteRange {
		&self.index_range
	}

	/// Returns the zoom level of the block.
	///
	/// # Returns
	/// The zoom level of the block.
	#[allow(dead_code)]
	pub fn get_z(&self) -> u8 {
		self.offset.level
	}

	/// Returns the coordinate of the block.
	///
	/// # Returns
	/// A reference to the coordinate of the block.
	pub fn get_coord(&self) -> &TileCoord {
		&self.offset
	}

	#[cfg(test)]
	pub fn as_str(&self) -> String {
		let x_offset = self.offset.x * 256;
		let y_offset = self.offset.y * 256;
		format!(
			"[{},[{},{}],[{},{}]]",
			self.offset.level,
			self.tiles_coverage.x_min() + x_offset,
			self.tiles_coverage.y_min() + y_offset,
			self.tiles_coverage.x_max() + x_offset,
			self.tiles_coverage.y_max() + y_offset
		)
	}
}

impl fmt::Debug for BlockDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("BlockDefinition")
			.field("x/y/z", &self.offset)
			.field("bbox", &self.tiles_coverage)
			.field("tiles_range", &self.tiles_range)
			.field("index_range", &self.index_range)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn multitest() -> Result<()> {
		let mut def = BlockDefinition::new(&TileBBox::from_boundaries(12, 300, 400, 320, 450)?)?;
		def.tiles_range = ByteRange::new(4, 5);
		def.index_range = ByteRange::new(9, 6);

		assert_eq!(def, BlockDefinition::from_blob(&def.as_blob()?)?);
		assert_eq!(def.count_tiles(), 1071);
		assert_eq!(def.as_blob()?.len(), 33);
		assert_eq!(def.get_sort_index(), 5596502);
		assert_eq!(def.as_str(), "[12,[300,400],[320,450]]");
		assert_eq!(def.get_z(), 12);
		assert_eq!(def.get_coord(), &TileCoord::new(12, 1, 1)?);
		assert_eq!(
			def.get_global_bbox(),
			&TileBBox::from_boundaries(12, 300, 400, 320, 450)?
		);
		assert_eq!(
			format!("{def:?}"),
			"BlockDefinition { x/y/z: TileCoord(12, [1, 1]), bbox: 8: [44,144,64,194] (21x51), tiles_range: ByteRange[4,5], index_range: ByteRange[9,6] }"
		);

		let def2 = BlockDefinition::from_blob(&def.as_blob()?)?;
		assert_eq!(def, def2);

		Ok(())
	}

	#[test]
	fn test_set_tiles_range() -> Result<()> {
		let bbox = TileBBox::from_boundaries(14, 0, 0, 255, 255)?;
		let mut def = BlockDefinition::new(&bbox)?;
		let range = ByteRange::new(10, 20);

		def.set_tiles_range(range);
		assert_eq!(*def.get_tiles_range(), range);

		Ok(())
	}

	#[test]
	fn test_set_index_range() -> Result<()> {
		let bbox = TileBBox::from_boundaries(14, 0, 0, 255, 255)?;
		let mut def = BlockDefinition::new(&bbox)?;
		let range = ByteRange::new(10, 20);

		def.set_index_range(range);
		assert_eq!(*def.get_index_range(), range);

		Ok(())
	}
}
