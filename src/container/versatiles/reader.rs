// Import necessary modules and traits
use super::types::{BlockDefinition, BlockIndex, FileHeader, TileIndex};
#[cfg(feature = "full")]
use crate::helper::pretty_print::PrettyPrint;
use crate::{
	container::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait, TilesStream},
	helper::{DataReaderFile, DataReaderTrait, TileConverter},
	types::{Blob, ByteRange, TileBBox, TileCompression, TileCoord2, TileCoord3},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use log::trace;
use std::{collections::HashMap, fmt::Debug, ops::Shr, path::Path, sync::Arc};
use tokio::sync::Mutex;

// Define the TilesReader struct
pub struct VersaTilesReader {
	meta: Option<Blob>,
	reader: Box<dyn DataReaderTrait>,
	parameters: TilesReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: HashMap<TileCoord3, Arc<TileIndex>>,
}

// Implement methods for the TilesReader struct
impl VersaTilesReader {
	// Create a new TilesReader from a given filename
	pub async fn open_path(path: &Path) -> Result<TilesReaderBox> {
		Self::open_reader(DataReaderFile::from_path(path)?).await
	}

	// Create a new TilesReader from a given data reader
	pub async fn open_reader(mut reader: Box<dyn DataReaderTrait>) -> Result<TilesReaderBox> {
		let header = FileHeader::from_reader(&mut reader)
			.await
			.context("reading the header")?;

		let meta = if header.meta_range.length > 0 {
			Some(
				TileConverter::new_decompressor(&header.compression)
					.process_blob(
						reader
							.read_range(&header.meta_range)
							.await
							.context("reading the meta data")?,
					)
					.context("decompressing the meta data")?,
			)
		} else {
			None
		};

		let block_index = BlockIndex::from_brotli_blob(
			reader
				.read_range(&header.blocks_range)
				.await
				.context("reading the block index")?,
		)
		.context("decompressing the block index")?;

		let bbox_pyramid = block_index.get_bbox_pyramid();
		let parameters = TilesReaderParameters::new(header.tile_format, header.compression, bbox_pyramid);

		Ok(Box::new(VersaTilesReader {
			meta,
			reader,
			parameters,
			block_index,
			tile_index_cache: HashMap::new(),
		}))
	}

	async fn get_block_tile_index_cached(&mut self, block: &BlockDefinition) -> Arc<TileIndex> {
		let block_coord = block.get_coord3();

		// Retrieve the tile index from cache or read from the reader
		let tile_index_option = self.tile_index_cache.get(block_coord);

		if let Some(tile_index) = tile_index_option {
			return tile_index.clone();
		}

		let tile_index = self.get_block_tile_index(block).await;

		self.tile_index_cache.insert(*block_coord, Arc::new(tile_index));

		return self.tile_index_cache.get(block_coord).unwrap().clone();
	}

	async fn get_block_tile_index(&mut self, block: &BlockDefinition) -> TileIndex {
		let blob = self.reader.read_range(block.get_index_range()).await.unwrap();
		let mut tile_index = TileIndex::from_brotli_blob(blob).unwrap();
		tile_index.add_offset(block.get_tiles_range().offset);

		assert_eq!(tile_index.len(), block.count_tiles() as usize);

		tile_index
	}
}

// Implement Send and Sync traits for TilesReader
unsafe impl Send for VersaTilesReader {}
unsafe impl Sync for VersaTilesReader {}

// Implement the TilesReaderTrait for the TilesReader struct
#[async_trait]
impl TilesReaderTrait for VersaTilesReader {
	// Get the container name
	fn get_container_name(&self) -> &str {
		"versatiles"
	}

	// Get metadata
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}

	// Get TilesReader parameters
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}

	// Get tile data for a given coordinate
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		// Calculate block coordinate
		let block_coord = TileCoord3::new(coord.get_x().shr(8), coord.get_y().shr(8), coord.get_z())?;

		// Get the block using the block coordinate
		let block = self.block_index.get_block(&block_coord);

		if block.is_none() {
			return Ok(None);
		}
		let block = block.unwrap().clone();

		// Get the block and its bounding box
		let bbox = block.get_global_bbox();

		// Calculate tile coordinates within the block
		let tile_coord: TileCoord2 = coord.as_coord2();

		// Check if the tile is within the block definition
		if !bbox.contains(&tile_coord) {
			trace!("tile {coord:?} outside block definition");
			return Ok(None);
		}

		// Get the tile ID
		let tile_id = bbox.get_tile_index(&tile_coord);

		// Retrieve the tile index from cache or read from the reader
		let tile_index: Arc<TileIndex> = self.get_block_tile_index_cached(&block).await;
		let tile_range: &ByteRange = tile_index.get(tile_id);

		//  None if the tile range has zero length
		if tile_range.length == 0 {
			return Ok(None);
		}

		// Read the tile data from the reader
		Ok(Some(self.reader.read_range(tile_range).await?))
	}

	async fn get_bbox_tile_stream<'a>(&'a mut self, bbox: &TileBBox) -> TilesStream<'a> {
		const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
		const MAX_CHUNK_GAP: u64 = 32 * 1024;

		let bbox = bbox.clone();

		let mut block_coords: TileBBox = bbox.clone();
		block_coords.scale_down(256);
		let block_coords: Vec<TileCoord3> = block_coords.iter_coords().collect();

		let self_mutex = Arc::new(Mutex::new(self));

		let chunks: Vec<Vec<Vec<(TileCoord3, ByteRange)>>> = futures::stream::iter(block_coords)
			.then(|block_coord: TileCoord3| {
				let bbox = bbox.clone();
				let self_mutex = self_mutex.clone();
				async move {
					let mut myself = self_mutex.lock().await;

					// Get the block using the block coordinate
					let block_option = myself.block_index.get_block(&block_coord);
					if block_option.is_none() {
						panic!("block <{block_coord:#?}> does not exist");
					}

					// Get the block
					let block: BlockDefinition = block_option.unwrap().to_owned();
					trace!("block {block:?}");

					// Get the bounding box of all tiles defined in this block
					let tiles_bbox_block = block.get_global_bbox();
					trace!("tiles_bbox_block {tiles_bbox_block:?}");

					// Get the bounding box of all tiles defined in this block
					let mut tiles_bbox_used: TileBBox = bbox.clone();
					tiles_bbox_used.intersect_bbox(tiles_bbox_block);
					trace!("tiles_bbox_used {tiles_bbox_used:?}");

					assert_eq!(bbox.level, tiles_bbox_block.level);
					assert_eq!(bbox.level, tiles_bbox_used.level);

					// Get the tile index of this block
					let tile_index: Arc<TileIndex> = myself.get_block_tile_index_cached(&block).await;
					trace!("tile_index {tile_index:?}");

					// let tile_range: &ByteRange = tile_index.get(tile_id);
					let mut tile_ranges: Vec<(TileCoord3, ByteRange)> = tile_index
						.iter()
						.enumerate()
						.map(|(index, range)| (tiles_bbox_block.get_coord3_by_index(index as u32).unwrap(), *range))
						.filter(|(coord, range)| tiles_bbox_used.contains3(coord) && (range.length > 0))
						.collect();

					tile_ranges.sort_by_key(|e| e.1.offset);

					let mut chunks: Vec<Vec<(TileCoord3, ByteRange)>> = Vec::new();
					let mut chunk: Vec<(TileCoord3, ByteRange)> = Vec::new();

					for entry in tile_ranges {
						if chunk.is_empty() {
							chunk.push(entry)
						} else {
							let newest = &entry.1;
							let first = &chunk.first().unwrap().1;
							let last = &chunk.last().unwrap().1;
							if (first.offset + MAX_CHUNK_SIZE > newest.offset + newest.length)
								&& (last.offset + last.length + MAX_CHUNK_GAP > newest.offset)
							{
								// chunk size is still inside the limits
								chunk.push(entry);
							} else {
								// chunk becomes to big
								chunks.push(chunk);
								chunk = Vec::new();
							}
						}
					}

					if !chunk.is_empty() {
						chunks.push(chunk);
					}

					chunks
				}
			})
			.collect()
			.await;

		let chunks: Vec<Vec<(TileCoord3, ByteRange)>> = chunks.into_iter().flatten().collect();

		stream::iter(chunks)
			.then(move |chunk| {
				let bbox = bbox.clone();
				let self_mutex = self_mutex.clone();
				async move {
					let mut myself = self_mutex.lock().await;

					let first = chunk.first().unwrap().1;
					let last = chunk.last().unwrap().1;

					let offset = first.offset;
					let end = last.offset + last.length;

					let chunk_range = ByteRange::new(offset, end - offset);

					let big_blob = myself.reader.read_range(&chunk_range).await.unwrap();

					let entries: Vec<(TileCoord3, Blob)> = chunk
						.into_iter()
						.map(|(coord, range)| {
							let start = range.offset - offset;
							let end = start + range.length;
							let tile_range = (start as usize)..(end as usize);

							let blob = Blob::from(big_blob.get_range(tile_range));

							assert!(bbox.contains3(&coord), "outer_bbox {bbox:?} does not contain {coord:?}");

							(coord, blob)
						})
						.collect();

					stream::iter(entries)
				}
			})
			.flatten()
			.boxed()
	}

	// Get the name of the reader
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}

	// deep probe of container meta
	#[cfg(feature = "full")]
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_key_value("meta size", &self.meta.as_ref().map_or(0, |b| b.len()))
			.await;
		print.add_key_value("block count", &self.block_index.len()).await;

		let mut index_size = 0;
		let mut tiles_size = 0;

		for block in self.block_index.iter() {
			index_size += block.get_index_range().length;
			tiles_size += block.get_tiles_range().length;
		}

		print.add_key_value("sum of block index sizes", &index_size).await;
		print.add_key_value("sum of block tiles sizes", &tiles_size).await;

		Ok(())
	}

	// deep probe of container tiles
	#[cfg(feature = "full")]
	async fn probe_tiles(&mut self, print: &PrettyPrint) -> Result<()> {
		#[derive(Debug)]
		#[allow(dead_code)]
		struct Entry {
			size: u64,
			x: u32,
			y: u32,
			z: u8,
		}

		let mut biggest_tiles: Vec<Entry> = Vec::new();
		let mut min_size: u64 = 0;
		let mut size_sum: u64 = 0;
		let mut tile_count: u64 = 0;

		let block_index = self.block_index.clone();
		let mut progress = crate::helper::progress_bar::ProgressBar::new("scanning blocks", block_index.len() as u64);

		for block in block_index.iter() {
			let tile_index = self.get_block_tile_index(block).await;
			for (index, tile_range) in tile_index.iter().enumerate() {
				let size = tile_range.length;

				tile_count += 1;
				size_sum += size;

				if size < min_size {
					continue;
				}

				let bbox = block.get_global_bbox();
				let coord = bbox.get_coord3_by_index(index as u32)?;

				biggest_tiles.push(Entry {
					size,
					x: coord.x,
					y: coord.y,
					z: coord.z,
				});
				biggest_tiles.sort_by(|a, b| b.size.cmp(&a.size));
				while biggest_tiles.len() > 10 {
					biggest_tiles.pop();
				}
				min_size = biggest_tiles.last().unwrap().size;
			}
			progress.inc(1);
		}
		progress.remove();

		print
			.add_key_value("average tile size", &size_sum.div_euclid(tile_count))
			.await;

		for (index, entry) in biggest_tiles.iter().enumerate() {
			print
				.add_key_value(&format!("#{} biggest tile", index + 1), entry)
				.await;
		}

		Ok(())
	}
}

// Implement Debug for TilesReader
impl Debug for VersaTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("VersaTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
#[cfg(feature = "full")]
mod tests {
	use super::*;
	use crate::{
		container::{make_test_file, mock::MOCK_BYTES_PBF},
		helper::decompress_gzip,
		types::TileFormat,
	};
	use anyhow::Result;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::PBF, TileCompression::Gzip, 4, "versatiles").await?;

		let mut reader = VersaTilesReader::open_path(&temp_file).await?;

		assert_eq!(format!("{:?}", reader), "VersaTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256)], tile_compression: Gzip, tile_format: PBF } }");
		assert_eq!(reader.get_container_name(), "versatiles");
		assert!(reader.get_name().ends_with(temp_file.to_str().unwrap()));
		assert_eq!(reader.get_meta().await?, Some(Blob::from(b"dummy meta data".to_vec())));
		assert_eq!(format!("{:?}", reader.get_parameters()), "TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256)], tile_compression: Gzip, tile_format: PBF }");
		assert_eq!(reader.get_parameters().tile_compression, TileCompression::Gzip);
		assert_eq!(reader.get_parameters().tile_format, TileFormat::PBF);

		let tile = reader.get_tile_data(&TileCoord3::new(15, 1, 4)?).await?.unwrap();
		assert_eq!(decompress_gzip(tile)?.as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}

	// Test tile fetching
	#[tokio::test]
	async fn probe() -> Result<()> {
		use crate::helper::pretty_print::PrettyPrint;

		let temp_file = make_test_file(TileFormat::PBF, TileCompression::Gzip, 4, "versatiles").await?;

		let mut reader = VersaTilesReader::open_path(&temp_file).await?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n   meta size: 15\n   block count: 5\n   sum of block index sizes: 70\n   sum of block tiles sizes: 385\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await.get(0..73).unwrap(),
			"tiles:\n   average tile size: 77\n   #1 biggest tile: Entry { size: 77, x: "
		);

		Ok(())
	}
}
