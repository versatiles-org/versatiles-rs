use super::types::{BlockIndex, CloudTilesSrc, FileHeader, TileIndex};
use crate::opencloudtiles::{
	containers::abstract_container::{TileReaderBox, TileReaderTrait},
	helpers::{decompress, decompress_brotli},
	types::{Blob, Precompression, TileCoord2, TileCoord3, TileReaderParameters},
};
use std::{collections::HashMap, fmt::Debug, ops::Shr, path::PathBuf, str::from_utf8};

pub struct TileReader {
	meta: Blob,
	reader: CloudTilesSrc,
	parameters: TileReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: HashMap<TileCoord3, TileIndex>,
}

impl TileReader {
	pub fn new(mut reader: CloudTilesSrc) -> TileReader {
		let header = FileHeader::from_reader(&mut reader);

		let meta = if header.meta_range.length > 0 {
			decompress_brotli(reader.read_range(&header.meta_range))
		} else {
			Blob::empty()
		};

		let block_index = BlockIndex::from_brotli_blob(reader.read_range(&header.blocks_range));
		let bbox_pyramide = block_index.get_bbox_pyramide();
		let parameters =
			TileReaderParameters::new(header.tile_format, header.precompression, bbox_pyramide);
		return TileReader {
			meta,
			reader,
			parameters,
			block_index,
			tile_index_cache: HashMap::new(),
		};
	}
}

unsafe impl Send for TileReader {}
unsafe impl Sync for TileReader {}

impl TileReaderTrait for TileReader {
	fn from_file(filename: &PathBuf) -> TileReaderBox {
		let reader = CloudTilesSrc::from_file(filename);
		return Box::new(TileReader::new(reader));
	}
	fn get_meta(&self) -> (Blob, Precompression) {
		return (
			self.meta.clone(),
			self.parameters.get_tile_precompression().clone(),
		);
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return &self.parameters;
	}
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<(Blob, Precompression)> {
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
			return None;
		}

		let block = block_option.unwrap();

		let tile_x = coord.x - block_coord.x * 256;
		let tile_y = coord.y - block_coord.y * 256;

		if !block.bbox.contains(&TileCoord2::new(tile_x, tile_y)) {
			println!("tile {:?} outside block definition", coord);
			return None;
		}

		let tile_index = self
			.tile_index_cache
			.entry(block_coord)
			.or_insert_with(|| TileIndex::from_brotli_blob(self.reader.read_range(&block.tile_range)));

		let tile_id = block.bbox.get_tile_index(&TileCoord2::new(tile_x, tile_y));

		let tile_range = tile_index.get_tile_range(tile_id as usize);

		return Some((
			self.reader.read_range(&tile_range),
			self.parameters.get_tile_precompression().clone(),
		));
	}
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let meta = self.get_meta();
		f.debug_struct("TileReader:CloudTiles")
			.field(
				"meta",
				&from_utf8(decompress(meta.0, &meta.1).as_slice()).unwrap(),
			)
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
