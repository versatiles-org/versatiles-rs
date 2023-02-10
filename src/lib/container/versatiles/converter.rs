use super::types::*;
use crate::{container::*, helper::*};
use log::{debug, trace};
use rayon::prelude::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use std::{collections::HashMap, path::Path, sync::Mutex};

pub struct TileConverter {
	writer: VersaTilesDst,
	config: TileConverterConfig,
}

impl TileConverterTrait for TileConverter {
	fn new(filename: &Path, tile_config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized,
	{
		Box::new(TileConverter {
			writer: VersaTilesDst::new_file(filename),
			config: tile_config,
		})
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		self.config.finalize_with_parameters(reader.get_parameters());

		let bbox_pyramide: &TileBBoxPyramide = self.config.get_bbox_pyramide();
		let mut header = FileHeader::new(
			self.config.get_tile_format(),
			self.config.get_tile_precompression(),
			[
				bbox_pyramide.get_zoom_min().unwrap(),
				bbox_pyramide.get_zoom_max().unwrap(),
			],
			bbox_pyramide.get_geo_bbox(),
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

		for (zoom, bbox_tiles) in self.config.get_bbox_pyramide().iter_levels() {
			let bbox_blocks = bbox_tiles.scale_down(256);
			for TileCoord2 { x, y } in bbox_blocks.iter_coords() {
				let mut bbox_block = *bbox_tiles;
				bbox_block.intersect_bbox(&TileBBox::new(x * 256, y * 256, x * 256 + 255, y * 256 + 255));

				blocks.push(BlockDefinition::new(x, y, zoom, bbox_block))
			}
		}

		let sum = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut progress = ProgressBar::new("converting tiles", sum);

		let mut block_index = BlockIndex::new_empty();

		for mut block in blocks.into_iter() {
			let (tiles_range, index_range) = self.write_block(&block, reader, &mut progress);

			if tiles_range.length + index_range.length == 0 {
				// block is empty
				continue;
			}

			block.tiles_range = tiles_range;
			block.index_range = index_range;

			block_index.add_block(block);
		}
		progress.finish();

		self.writer.append(&block_index.as_brotli_blob())
	}
	fn write_block(
		&mut self, block: &BlockDefinition, reader: &TileReaderBox, progress: &mut ProgressBar,
	) -> (ByteRange, ByteRange) {
		debug!("start block {:?}", block);

		let offset0 = self.writer.get_position();

		let bbox = &block.bbox;
		let mut tile_index = TileIndex::new_empty(bbox.count_tiles() as usize);
		let tile_hash_lookup: HashMap<Vec<u8>, ByteRange> = HashMap::new();

		let mutex_progress = &Mutex::new(progress);
		let mutex_writer = &Mutex::new(&mut self.writer);
		let mutex_tile_index = &Mutex::new(&mut tile_index);
		let mutex_tile_hash_lookup = &Mutex::new(tile_hash_lookup);

		let tile_converter = self.config.get_tile_recompressor();
		let width = 2u64.pow(block.z as u32);

		bbox
			.iter_bbox_row_slices(1024)
			.par_bridge()
			.for_each(|row_bbox: TileBBox| {
				trace!("start block slice {:?}", row_bbox);

				let mut blobs: Vec<(TileCoord2, Blob)> = reader.get_bbox_tile_vec(block.z, &row_bbox);

				blobs.sort_by_cached_key(|(coord, _blob)| coord.y * width + coord.x);

				trace!(
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

				trace!(
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
							secured_tile_index.set(index, *secured_tile_hash_lookup.get(blob.as_slice()).unwrap());
							return;
						}
						tile_hash_option = Some(blob.clone());
					}

					let mut range = secured_writer.append(blob);
					range.offset -= offset0;
					secured_tile_index.set(index, range);

					if let Some(tile_hash) = tile_hash_option {
						secured_tile_hash_lookup.insert(tile_hash.as_vec(), range);
					}
				});

				mutex_progress.lock().unwrap().inc(row_bbox.count_tiles());

				trace!("finish block slice {:?}", row_bbox);
			});

		debug!("finish block and write index {:?}", block);

		let offset1 = self.writer.get_position();
		let index_range = self.writer.append(&tile_index.as_brotli_blob());

		(ByteRange::new(offset0, offset1 - offset0), index_range)
	}
}
