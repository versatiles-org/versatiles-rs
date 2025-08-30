use super::{BlockDefinition, TileIndex};
use anyhow::Result;
use std::collections::HashMap;
use versatiles_core::{Blob, ByteRange, TileBBox, TileCoord, io::DataWriterTrait};
use versatiles_derive::context;

pub struct BlockWriter<'a> {
	pub bbox: TileBBox,
	writer: &'a mut dyn DataWriterTrait,
	initial_offset: u64,
	tile_index: TileIndex,
	tile_hash_lookup: HashMap<Vec<u8>, ByteRange>,
}

impl<'a> BlockWriter<'a> {
	pub fn new(block_definition: &BlockDefinition, writer: &'a mut dyn DataWriterTrait) -> Self {
		let bbox = *block_definition.get_global_bbox();
		let initial_offset = writer.get_position().unwrap();
		let tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		Self {
			bbox,
			writer,
			initial_offset,
			tile_index,
			tile_hash_lookup,
		}
	}

	/// Write a single tile to the writer.
	#[context("writing tile at {coord:?}")]
	pub fn write_tile(&mut self, coord: TileCoord, blob: Blob) -> Result<()> {
		let index = self.bbox.get_tile_index(&coord)? as usize;

		let mut save_hash = false;
		if blob.len() < 1000 {
			if let Some(range) = self.tile_hash_lookup.get(blob.as_slice()) {
				self.tile_index.set(index, *range);
				return Ok(());
			}
			save_hash = true;
		}

		let mut range = self.writer.append(&blob)?;
		range.shift_backward(self.initial_offset);

		self.tile_index.set(index, range);

		if save_hash {
			self.tile_hash_lookup.insert(blob.into_vec(), range);
		}

		Ok(())
	}

	#[context("finalizing block writer")]
	pub fn finalize(self) -> Result<(ByteRange, ByteRange)> {
		// Get the final writer position
		let offset1 = self.writer.get_position()?;
		let tile_range = ByteRange::new(self.initial_offset, offset1 - self.initial_offset);
		let index_range = self.writer.append(&self.tile_index.as_brotli_blob()?)?;

		Ok((tile_range, index_range))
	}
}
