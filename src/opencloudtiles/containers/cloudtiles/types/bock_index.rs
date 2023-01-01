use super::BlockDefinition;
use crate::opencloudtiles::{
	compress::{compress_brotli, decompress_brotli},
	types::{TileBBoxPyramide, TileCoord3},
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
		return BlockIndex {
			lookup: HashMap::new(),
		};
	}
	pub fn from_vec(buf: &Vec<u8>) -> BlockIndex {
		let count = buf.len().div(29);
		assert_eq!(
			count * 29,
			buf.len(),
			"block index is defect, cause buffer length is not a multiple of 29"
		);
		let mut block_index = BlockIndex::new_empty();
		for i in 0..count {
			block_index.add_block(BlockDefinition::from_vec(&buf[i * 29..(i + 1) * 29]))
		}
		return block_index;
	}
	pub fn from_brotli_vec(buf: &Vec<u8>) -> BlockIndex {
		let temp = &decompress_brotli(buf);
		return BlockIndex::from_vec(temp);
	}
	pub fn get_bbox_pyramide(&self) -> TileBBoxPyramide {
		let mut pyramide = TileBBoxPyramide::new_empty();
		for (_coord, block) in self.lookup.iter() {
			pyramide.include_bbox(block.level, &block.bbox);
		}
		return pyramide;
	}
	pub fn add_block(&mut self, block: BlockDefinition) {
		self
			.lookup
			.insert(TileCoord3::new(block.level, block.y, block.x), block);
	}
	pub fn as_vec(&self) -> Vec<u8> {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		for (_coord, block) in self.lookup.iter() {
			let vec = block.as_vec();
			let slice = vec.as_slice();
			//println!("{}", slice.len());
			cursor.write(slice).unwrap();
		}
		return cursor.into_inner();
	}
	pub fn as_brotli_vec(&self) -> Vec<u8> {
		return compress_brotli(&self.as_vec());
	}
	pub fn get_block(&self, coord: &TileCoord3) -> Option<&BlockDefinition> {
		return self.lookup.get(coord);
	}
}
