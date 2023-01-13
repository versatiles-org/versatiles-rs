use super::types::*;
use crate::opencloudtiles::{container::*, lib::*};
use log::{debug, trace};
use rayon::prelude::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use std::{collections::HashMap, path::Path, sync::Mutex};

pub struct TileConverter {
	writer: CloudTilesDst,
	config: TileConverterConfig,
}

impl TileConverterTrait for TileConverter {
	fn new(filename: &Path, tile_config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized,
	{
		Box::new(TileConverter {
			writer: CloudTilesDst::new_file(filename),
			config: tile_config,
		})
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		self
			.config
			.finalize_with_parameters(reader.get_parameters());

		let mut header = FileHeader::new(
			self.config.get_tile_format(),
			self.config.get_tile_precompression(),
		);
		self.writer.append(&header.to_blob());

		header.meta_range = self.write_meta(reader);
		header.blocks_range = self.write_blocks(reader);

		self.writer.write_start(&header.to_blob())
	}
}

impl TileConverter {
	fn write_meta(&mut self, reader: &TileReaderBox) -> ByteRange {
		let meta = reader.get_meta();
		let compressed = self.config.get_compressor().run(meta);

		self.writer.append(&compressed)
	}
	fn write_blocks(&mut self, reader: &mut TileReaderBox) -> ByteRange {
		let pyramide = self.config.get_bbox_pyramide();
		if pyramide.is_empty() {
			return ByteRange::empty();
		}

		let mut blocks: Vec<BlockDefinition> = Vec::new();
		let mut bar1 = ProgressBar::new("counting tiles", self.config.get_max_zoom().unwrap());

		for (zoom, bbox_tiles) in self.config.get_bbox_pyramide().iter_levels() {
			bar1.set_position(zoom);

			let bbox_blocks = bbox_tiles.clone().scale_down(256);
			for TileCoord2 { x, y } in bbox_blocks.iter_coords() {
				let mut bbox_block = bbox_tiles.clone();
				bbox_block.intersect_bbox(&TileBBox::new(
					x * 256,
					y * 256,
					x * 256 + 255,
					y * 256 + 255,
				));

				blocks.push(BlockDefinition::new(zoom, x, y, bbox_block))
			}
		}
		bar1.finish();

		let sum = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut bar2 = ProgressBar::new("converting tiles", sum);

		let mut block_index = BlockIndex::new_empty();

		for mut block in blocks.into_iter() {
			let range = self.write_block(&block, reader, &mut bar2);

			if range.length == 0 {
				// block is empty
				continue;
			}

			block.tile_range = range;
			block_index.add_block(block);
		}
		bar2.finish();

		self.writer.append(&block_index.as_brotli_blob())
	}
	fn write_block(
		&mut self, block: &BlockDefinition, reader: &TileReaderBox, bar: &mut ProgressBar,
	) -> ByteRange {
		debug!("start block {:?}", block);

		let bbox = &block.bbox;
		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let mutex_bar = &Mutex::new(bar);
		let mutex_writer = &Mutex::new(&mut self.writer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		let tile_converter = self.config.get_tile_recompressor();

		bbox
			.iter_bbox_row_slices(2048)
			.par_bridge()
			.for_each(|row_bbox: TileBBox| {
				debug!("start block slice {:?}", row_bbox);

				let mut blobs: Vec<(TileCoord2, Blob)> =
					reader.get_bbox_tile_vec(block.level, &row_bbox);

				debug!(
					"get_bbox_tile_vec: count {}, size sum {}",
					blobs.len(),
					blobs.iter().fold(0, |acc, e| acc + e.1.len())
				);

				if !tile_converter.is_empty() {
					blobs = blobs
						.par_iter()
						.map(|(coord, blob)| (coord.clone(), tile_converter.run(blob.clone())))
						.collect();
				}

				debug!(
					"compressed: count {}, size sum {}",
					blobs.len(),
					blobs.iter().fold(0, |acc, e| acc + e.1.len())
				);

				let mut secured_tile_hash_lookup = mutex_tile_hash_lookup.lock().unwrap();
				let mut secured_tile_index = mutex_tile_index.lock().unwrap();
				let mut secured_writer = mutex_writer.lock().unwrap();

				blobs.iter().for_each(|(coord, blob)| {
					trace!("blob size {}", blob.len());

					let index = bbox.get_tile_index(coord);

					let mut tile_hash_option = None;

					if blob.len() < 1000 {
						if secured_tile_hash_lookup.contains_key(blob.as_slice()) {
							secured_tile_index.set(
								index,
								secured_tile_hash_lookup
									.get(blob.as_slice())
									.unwrap()
									.clone(),
							);
							return;
						}
						tile_hash_option = Some(blob.clone());
					}

					let range = secured_writer.append(blob);
					secured_tile_index.set(index, range.clone());

					if let Some(tile_hash) = tile_hash_option {
						secured_tile_hash_lookup.insert(tile_hash.to_vec(), range);
					}
				});

				mutex_bar.lock().unwrap().inc(row_bbox.count_tiles());

				debug!("finish block slice {:?}", row_bbox);
			});

		debug!("finish block and write index {:?}", block);

		self.writer.append(&tile_index.as_brotli_blob())
	}
}
