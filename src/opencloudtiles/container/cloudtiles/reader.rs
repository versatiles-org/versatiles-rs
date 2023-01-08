use super::types::{
	BlockIndex, ByteRange, CloudTilesSrc, CloudTilesSrcTrait, FileHeader, TileIndex,
};
use crate::opencloudtiles::{
	container::{TileReaderBox, TileReaderTrait},
	lib::*,
};
use std::{collections::HashMap, fmt::Debug, ops::Shr, str::from_utf8, sync::RwLock};

pub struct TileReader {
	meta: Blob,
	reader: CloudTilesSrc,
	parameters: TileReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: RwLock<HashMap<TileCoord3, TileIndex>>,
}

impl TileReader {
	pub fn from_src(mut reader: CloudTilesSrc) -> TileReader {
		let header = FileHeader::from_reader(&mut reader);

		let meta = if header.meta_range.length > 0 {
			DataConverter::new_decompressor(&header.precompression)
				.run(reader.read_range(&header.meta_range))
		} else {
			Blob::empty()
		};

		let block_index = BlockIndex::from_brotli_blob(reader.read_range(&header.blocks_range));
		let bbox_pyramide = block_index.get_bbox_pyramide();
		let parameters =
			TileReaderParameters::new(header.tile_format, header.precompression, bbox_pyramide);

		TileReader {
			meta,
			reader,
			parameters,
			block_index,
			tile_index_cache: RwLock::new(HashMap::new()),
		}
	}
}

unsafe impl Send for TileReader {}
unsafe impl Sync for TileReader {}

impl TileReaderTrait for TileReader {
	fn new(filename: &str) -> TileReaderBox {
		let reader = CloudTilesSrc::new(filename);

		Box::new(TileReader::from_src(reader))
	}
	fn get_meta(&self) -> Blob {
		self.meta.clone()
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		&self.parameters
	}
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob> {
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

		let tile_id = block.bbox.get_tile_index(&TileCoord2::new(tile_x, tile_y));

		let cache_reader = self.tile_index_cache.read().unwrap();
		let tile_index_option = cache_reader.get(&block_coord);

		let tile_range: ByteRange;

		if let Some(tile_index) = tile_index_option {
			tile_range = tile_index.get_tile_range(tile_id).clone();

			drop(cache_reader);
		} else {
			drop(cache_reader);

			let tile_index = TileIndex::from_brotli_blob(self.reader.read_range(&block.tile_range));
			let mut cache_writer = self.tile_index_cache.write().unwrap();
			cache_writer.insert(block_coord.clone(), tile_index);

			drop(cache_writer);

			let cache_reader = self.tile_index_cache.read().unwrap();
			let tile_index_option = cache_reader.get(&block_coord);

			tile_range = tile_index_option
				.unwrap()
				.get_tile_range(tile_id)
				.clone();

			drop(cache_reader);
		}

		Some(self.reader.read_range(&tile_range))
	}
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:CloudTiles")
			.field("meta", &from_utf8(self.get_meta().as_slice()).unwrap())
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
