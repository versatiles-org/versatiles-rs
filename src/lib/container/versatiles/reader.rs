use super::types::*;
use crate::{container::*, helper::*};
use itertools::Itertools;
use log::debug;
use std::{collections::HashMap, fmt::Debug, ops::Shr, sync::RwLock};

pub struct TileReader {
	meta: Blob,
	reader: Box<dyn VersaTilesSrcTrait>,
	parameters: TileReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: RwLock<HashMap<TileCoord3, TileIndex>>,
}

impl TileReader {
	pub fn from_src(mut reader: Box<dyn VersaTilesSrcTrait>) -> TileReader {
		let header = FileHeader::from_reader(&mut reader);

		let meta = if header.meta_range.length > 0 {
			DataConverter::new_decompressor(&header.precompression).run(reader.read_range(&header.meta_range))
		} else {
			Blob::empty()
		};

		let block_index = BlockIndex::from_brotli_blob(reader.read_range(&header.blocks_range));
		let bbox_pyramide = block_index.get_bbox_pyramide();
		let parameters = TileReaderParameters::new(header.tile_format, header.precompression, bbox_pyramide);

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
		let reader = new_versatiles_src(filename);

		Box::new(TileReader::from_src(reader))
	}
	fn get_meta(&self) -> Blob {
		self.meta.clone()
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		&self.parameters
	}
	fn get_parameters_mut(&mut self) -> &mut TileReaderParameters {
		&mut self.parameters
	}
	fn get_tile_data(&self, coord_in: &TileCoord3) -> Option<Blob> {
		let coord: TileCoord3 = if self.get_parameters().get_vertical_flip() {
			coord_in.flip_vertically()
		} else {
			coord_in.to_owned()
		};

		let block_coord = TileCoord3 {
			x: coord.x.shr(8),
			y: coord.y.shr(8),
			z: coord.z,
		};

		let block_option = self.block_index.get_block(&block_coord);
		if block_option.is_none() {
			println!("block <{block_coord:#?}> for tile <{coord:#?}> does not exist");
			return None;
		}

		let block = block_option.unwrap();

		let tile_x = coord.x - block_coord.x * 256;
		let tile_y = coord.y - block_coord.y * 256;

		if !block.bbox.contains(&TileCoord2::new(tile_x, tile_y)) {
			println!("tile {coord:?} outside block definition");
			return None;
		}

		let tile_id = block.bbox.get_tile_index(&TileCoord2::new(tile_x, tile_y));

		let cache_reader = self.tile_index_cache.read().unwrap();
		let tile_index_option = cache_reader.get(&block_coord);

		let tile_range: ByteRange;

		if let Some(tile_index) = tile_index_option {
			tile_range = *tile_index.get_tile_range(tile_id);

			drop(cache_reader);
		} else {
			drop(cache_reader);

			let mut tile_index = TileIndex::from_brotli_blob(self.reader.read_range(&block.index_range));
			tile_index.add_offset(block.tiles_range.offset);

			let mut cache_writer = self.tile_index_cache.write().unwrap();
			cache_writer.insert(block_coord, tile_index);

			drop(cache_writer);

			let cache_reader = self.tile_index_cache.read().unwrap();
			let tile_index_option = cache_reader.get(&block_coord);

			tile_range = *tile_index_option.unwrap().get_tile_range(tile_id);

			drop(cache_reader);
		}

		Some(self.reader.read_range(&tile_range))
	}
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}
	fn deep_verify(&self) {
		let block_count = self.block_index.len() as u64;

		debug!("number of blocks: {}", block_count);

		let mut progress = ProgressBar::new("deep verify", self.block_index.get_bbox_pyramide().count_tiles());

		let blocks = self
			.block_index
			.iter()
			.sorted_by_cached_key(|block| block.get_sort_index());

		let mut status_images = StatusImagePyramide::new();

		for block in blocks {
			let tiles_count = block.bbox.count_tiles();

			let tile_index = TileIndex::from_brotli_blob(self.reader.read_range(&block.index_range));
			assert_eq!(tile_index.len(), tiles_count as usize, "tile count are not the same");

			let status_image = status_images.get_level(block.z);

			let x_offset = block.x * 256;
			let y_offset = block.y * 256;

			for (index, byterange) in tile_index.iter().enumerate() {
				let coord = block.bbox.get_coord_by_index(index);
				status_image.set(coord.x + x_offset, coord.y + y_offset, byterange.length);
			}

			progress.inc(block.count_tiles());
		}
		progress.finish();

		status_images.save("tile_sizes.png");
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:VersaTiles")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
