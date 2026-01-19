//! Build a block by streaming tiles and deferring TileIndex creation until finalize.
//!
//! Unlike `BlockWriter`, this struct does not require the bbox upfront.
//! It tracks the actual tile coverage while writing tiles, and creates
//! an optimally-sized TileIndex on finalize.

use super::{BlockDefinition, TileIndex};
use anyhow::Result;
use std::collections::HashMap;
use versatiles_core::{Blob, ByteRange, TileBBox, TileCoord, io::DataWriterTrait};
use versatiles_derive::context;

/// Builds a block by streaming tiles with deferred bbox/index calculation.
///
/// Tiles are written immediately to the underlying writer. Only lightweight
/// metadata (coordinates and byte ranges) is kept in memory. The actual bbox
/// is tracked incrementally, and the TileIndex is created at finalize time
/// with the optimal size for the actual tile coverage.
pub struct BlockBuilder<'a> {
	writer: &'a mut dyn DataWriterTrait,
	initial_offset: u64,
	tile_positions: Vec<(TileCoord, ByteRange)>,
	actual_bbox: TileBBox,
	tile_hash_lookup: HashMap<Vec<u8>, ByteRange>,
}

impl<'a> BlockBuilder<'a> {
	/// Create a new BlockBuilder for the given zoom level.
	///
	/// # Arguments
	/// * `level` - The zoom level for this block (0..=31)
	/// * `writer` - The data writer to write tiles to
	#[context("creating BlockBuilder for level {level}")]
	pub fn new(level: u8, writer: &'a mut dyn DataWriterTrait) -> Result<Self> {
		let initial_offset = writer.get_position()?;
		let actual_bbox = TileBBox::new_empty(level)?;

		Ok(Self {
			writer,
			initial_offset,
			tile_positions: Vec::new(),
			actual_bbox,
			tile_hash_lookup: HashMap::new(),
		})
	}

	/// Write a single tile to the block.
	///
	/// The tile is written immediately to the underlying writer. Only the
	/// coordinate and byte range are stored in memory.
	///
	/// # Arguments
	/// * `coord` - The tile coordinate
	/// * `blob` - The compressed tile data
	#[context("writing tile at {coord:?}")]
	pub fn write_tile(&mut self, coord: TileCoord, blob: Blob) -> Result<()> {
		// Track actual bbox
		self.actual_bbox.include_coord(&coord)?;

		// Deduplication for small tiles
		let range = if blob.len() < 1000 {
			if let Some(&existing) = self.tile_hash_lookup.get(blob.as_slice()) {
				existing
			} else {
				let mut range = self.writer.append(&blob)?;
				range.shift_backward(self.initial_offset);
				self.tile_hash_lookup.insert(blob.into_vec(), range);
				range
			}
		} else {
			let mut range = self.writer.append(&blob)?;
			range.shift_backward(self.initial_offset);
			range
		};

		self.tile_positions.push((coord, range));
		Ok(())
	}

	/// Finalize the block and return the BlockDefinition.
	///
	/// Creates an optimally-sized TileIndex based on the actual tile coverage,
	/// writes it to the underlying writer, and returns the completed BlockDefinition.
	///
	/// # Returns
	/// * `Ok(Some(BlockDefinition))` - If tiles were written
	/// * `Ok(None)` - If no tiles were written (empty block)
	#[context("finalizing block builder")]
	pub fn finalize(self) -> Result<Option<BlockDefinition>> {
		// Early return if empty
		if self.tile_positions.is_empty() {
			return Ok(None);
		}

		// Calculate tile range
		let tiles_end_offset = self.writer.get_position()?;
		let tiles_range = ByteRange::new(self.initial_offset, tiles_end_offset - self.initial_offset);

		// Create optimally-sized TileIndex
		let tile_count = usize::try_from(self.actual_bbox.count_tiles())?;
		let mut tile_index = TileIndex::new_empty(tile_count);
		for (coord, range) in self.tile_positions {
			let index = usize::try_from(self.actual_bbox.index_of(&coord)?)?;
			tile_index.set(index, range);
		}

		// Write index
		let index_range = self.writer.append(&tile_index.as_brotli_blob()?)?;

		// Create BlockDefinition with actual bbox
		let mut block = BlockDefinition::new(&self.actual_bbox)?;
		block.set_tiles_range(tiles_range);
		block.set_index_range(index_range);

		Ok(Some(block))
	}
}
