use super::BlockDefinition;
use crate::shared::{compress_brotli, decompress_brotli, Blob, TileBBoxPyramid, TileCoord3};
use std::{
	collections::HashMap,
	io::{Cursor, Write},
	ops::Div,
};

const BLOCK_INDEX_LENGTH: usize = 33;

#[derive(Debug, PartialEq, Eq)]
pub struct BlockIndex {
	lookup: HashMap<TileCoord3, BlockDefinition>,
}

impl BlockIndex {
	pub fn new_empty() -> Self {
		Self { lookup: HashMap::new() }
	}

	pub fn from_blob(buf: Blob) -> Self {
		let count = buf.len().div(BLOCK_INDEX_LENGTH);
		assert_eq!(
			count * BLOCK_INDEX_LENGTH,
			buf.len(),
			"Block index is defective, because buffer length is not a multiple of {}",
			BLOCK_INDEX_LENGTH
		);

		let mut block_index = Self::new_empty();
		for i in 0..count {
			block_index.add_block(
				BlockDefinition::from_slice(buf.get_range(i * BLOCK_INDEX_LENGTH..(i + 1) * BLOCK_INDEX_LENGTH)).unwrap(),
			);
		}

		block_index
	}

	pub fn from_brotli_blob(buf: Blob) -> Self {
		Self::from_blob(decompress_brotli(buf).unwrap())
	}

	pub fn get_bbox_pyramid(&self) -> TileBBoxPyramid {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for (_coord, block) in self.lookup.iter() {
			pyramid.include_bbox(block.get_z(), &block.get_bbox());
		}

		pyramid
	}

	pub fn add_block(&mut self, block: BlockDefinition) {
		self.lookup.insert(block.get_coord3(), block);
	}

	pub fn as_blob(&self) -> Blob {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		for (_coord, block) in self.lookup.iter() {
			cursor.write_all(&block.as_vec().unwrap()).unwrap();
		}

		Blob::from(cursor.into_inner())
	}

	pub fn as_brotli_blob(&self) -> Blob {
		compress_brotli(self.as_blob()).unwrap()
	}

	pub fn get_block(&self, coord: &TileCoord3) -> Option<&BlockDefinition> {
		self.lookup.get(coord)
	}

	pub fn len(&self) -> usize {
		self.lookup.len()
	}

	pub fn iter(&self) -> impl Iterator<Item = &BlockDefinition> {
		self.lookup.values()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::shared::TileBBox;

	#[test]
	fn conversion() {
		let mut index1 = BlockIndex::new_empty();
		index1.add_block(BlockDefinition::new(1, 2, 3, TileBBox::new_empty(3)));
		let index2 = BlockIndex::from_brotli_blob(index1.as_brotli_blob());
		assert_eq!(index1, index2);
	}
}
