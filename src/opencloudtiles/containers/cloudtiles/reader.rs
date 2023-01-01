use super::types::{BlockIndex, CloudTilesSrc, FileHeader, TileIndex};
use crate::opencloudtiles::{
	containers::abstract_container::{self, TileReaderBox},
	types::{TileCoord3, TileData, TileReaderParameters},
};
use std::{collections::HashMap, ops::Shr, path::PathBuf};

pub struct TileReader {
	reader: CloudTilesSrc,
	parameters: TileReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: HashMap<TileCoord3, TileIndex>,
}

impl TileReader {
	pub fn new(mut reader: CloudTilesSrc) -> TileReader {
		let header = FileHeader::read(&mut reader);
		println!("{:?}", header);
		let block_index = BlockIndex::from_brotli_vec(&reader.read_range(&header.blocks_range));
		let bbox_pyramide = block_index.get_bbox_pyramide();
		let parameters = TileReaderParameters::new(header.tile_format, bbox_pyramide);
		return TileReader {
			reader,
			parameters,
			block_index,
			tile_index_cache: HashMap::new(),
		};
	}
}

unsafe impl Send for TileReader {}
unsafe impl Sync for TileReader {}

impl abstract_container::TileReaderTrait for TileReader {
	fn from_file(filename: &PathBuf) -> TileReaderBox {
		let reader = CloudTilesSrc::from_file(filename);
		return Box::new(TileReader::new(reader));
	}
	fn get_meta(&self) -> &[u8] {
		panic!();
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return &self.parameters;
	}
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<TileData> {
		let block_coord = TileCoord3 {
			x: coord.x.shr(8),
			y: coord.y.shr(8),
			z: coord.z,
		};

		let block_option = self.block_index.get_block(&block_coord);
		if block_option.is_none() {
			println!(
				"block <{:#?}> for tile <{:#?}> does not exist",
				block_coord, coord
			);
			println!("<{:#?}>", self.block_index);
			panic!();
			return None;
		}

		let block = block_option.unwrap();

		let tile_x = coord.x - block_coord.x * 256;
		let tile_y = coord.y - block_coord.y * 256;
		if (tile_x < block.bbox.x_min) || (tile_y < block.bbox.y_min) {
			println!("tile {:?} outside block definition", coord);
			return None;
		}
		if (tile_x > block.bbox.x_max) || (tile_y > block.bbox.y_max) {
			println!("tile {:?} outside block definition", coord);
			return None;
		}

		let tile_index = self
			.tile_index_cache
			.entry(block_coord)
			.or_insert_with(|| TileIndex::from_brotli_vec(&self.reader.read_range(&block.tile_range)));

		let x = tile_x - block.bbox.x_min;
		let y = tile_y - block.bbox.y_min;
		let tile_id = y * (block.bbox.x_max - block.bbox.x_min + 1) + x;

		let tile_range = tile_index.get_tile_range(tile_id as usize);

		if tile_range.length == 0 {
			println!("tile_range not specified {:?}", coord);
			return None;
		}

		return Some(self.reader.read_range(&tile_range));
	}
}
