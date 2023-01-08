use super::BlockDefinition;
use crate::opencloudtiles::lib::{
	compress_brotli, decompress_brotli, Blob, TileBBoxPyramide, TileCoord3,
};
use std::{
	collections::HashMap,
	io::{Cursor, Write},
	ops::Div,
};

#[derive(Debug)]

pub struct BlockIndex {
	lookup: HashMap<TileCoord3, BlockDefinition>,
}
impl BlockIndex {
	pub fn new_empty() -> BlockIndex {
		BlockIndex {
			lookup: HashMap::new(),
		}
	}
	pub fn from_blob(buf: Blob) -> BlockIndex {
		let count = buf.len().div(29);
		assert_eq!(
			count * 29,
			buf.len(),
			"block index is defect, cause buffer length is not a multiple of 29"
		);
		let mut block_index = BlockIndex::new_empty();
		for i in 0..count {
			block_index.add_block(BlockDefinition::from_blob(
				buf.get_range(i * 29..(i + 1) * 29),
			));
		}
		
		block_index
	}
	pub fn from_brotli_blob(buf: Blob) -> BlockIndex {
		BlockIndex::from_blob(decompress_brotli(buf))
	}
	pub fn get_bbox_pyramide(&self) -> TileBBoxPyramide {
		let mut pyramide = TileBBoxPyramide::new_empty();
		for (_coord, block) in self.lookup.iter() {
			pyramide.include_bbox(
				block.level,
				&block.bbox.clone().shift_by(block.x * 256, block.y * 256),
			);
		}

		pyramide
	}
	pub fn add_block(&mut self, block: BlockDefinition) {
		self
			.lookup
			.insert(TileCoord3::new(block.level, block.y, block.x), block);
	}
	pub fn as_blob(&self) -> Blob {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		for (_coord, block) in self.lookup.iter() {
			cursor.write_all(block.as_blob().as_slice()).unwrap();
		}

		Blob::from_vec(cursor.into_inner())
	}
	pub fn as_brotli_blob(&self) -> Blob {
		compress_brotli(self.as_blob())
	}
	pub fn get_block(&self, coord: &TileCoord3) -> Option<&BlockDefinition> {
		self.lookup.get(coord)
	}
}
