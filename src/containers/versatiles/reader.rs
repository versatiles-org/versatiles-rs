// Import necessary modules and traits
use super::{new_data_reader, types::*, DataReaderTrait};
#[cfg(feature = "full")]
use crate::shared::PrettyPrint;
use crate::{
	containers::{TileReaderBox, TileReaderTrait, TileStream},
	create_error,
	shared::{Blob, DataConverter, Result, TileBBox, TileCoord2, TileCoord3, TileReaderParameters},
};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use log::trace;
use std::{collections::HashMap, fmt::Debug, ops::Shr, sync::Arc};
use tokio::sync::Mutex;

// Define the TileReader struct
pub struct TileReader {
	meta: Blob,
	reader: Box<dyn DataReaderTrait>,
	parameters: TileReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: HashMap<TileCoord3, Arc<TileIndex>>,
}

// Implement methods for the TileReader struct
impl TileReader {
	// Create a new TileReader from a given data reader
	pub async fn from_src(mut reader: Box<dyn DataReaderTrait>) -> Result<TileReader> {
		let header = FileHeader::from_reader(&mut reader).await?;

		let meta = if header.meta_range.length > 0 {
			DataConverter::new_decompressor(&header.compression)
				.process_blob(reader.read_range(&header.meta_range).await?)?
		} else {
			Blob::empty()
		};

		let block_index = BlockIndex::from_brotli_blob(reader.read_range(&header.blocks_range).await?);
		let bbox_pyramid = block_index.get_bbox_pyramid();
		let parameters = TileReaderParameters::new(header.tile_format, header.compression, bbox_pyramid);

		Ok(TileReader {
			meta,
			reader,
			parameters,
			block_index,
			tile_index_cache: HashMap::new(),
		})
	}

	async fn get_block_tile_index_cached(&mut self, block: &BlockDefinition) -> Arc<TileIndex> {
		let block_coord = block.get_coord3();

		// Retrieve the tile index from cache or read from the reader
		let tile_index_option = self.tile_index_cache.get(block_coord);

		if let Some(tile_index) = tile_index_option {
			return tile_index.clone();
		}

		let blob = self.reader.read_range(block.get_index_range()).await.unwrap();
		let mut tile_index = TileIndex::from_brotli_blob(blob);
		tile_index.add_offset(block.get_tiles_range().offset);

		assert_eq!(tile_index.len(), block.count_tiles() as usize);

		self.tile_index_cache.insert(*block_coord, Arc::new(tile_index));

		return self.tile_index_cache.get(block_coord).unwrap().clone();
	}
}

// Implement Send and Sync traits for TileReader
unsafe impl Send for TileReader {}
unsafe impl Sync for TileReader {}

// Implement the TileReaderTrait for the TileReader struct
#[async_trait]
impl TileReaderTrait for TileReader {
	// Create a new TileReader from a given filename
	async fn new(filename: &str) -> Result<TileReaderBox> {
		let source = new_data_reader(filename).await?;
		let reader = TileReader::from_src(source).await?;

		Ok(Box::new(reader))
	}

	// Get the container name
	fn get_container_name(&self) -> Result<&str> {
		Ok("versatiles")
	}

	// Get metadata
	async fn get_meta(&self) -> Result<Blob> {
		Ok(self.meta.clone())
	}

	// Get TileReader parameters
	fn get_parameters(&self) -> Result<&TileReaderParameters> {
		Ok(&self.parameters)
	}

	// Get mutable TileReader parameters
	fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters> {
		Ok(&mut self.parameters)
	}

	// Get tile data for a given coordinate
	async fn get_tile_data(&mut self, coord_in: &TileCoord3) -> Result<Blob> {
		let mut coord: TileCoord3 = *coord_in;

		if self.get_parameters()?.get_swap_xy() {
			coord.swap_xy();
		};

		if self.get_parameters()?.get_flip_y() {
			coord.flip_y();
		};

		// Calculate block coordinate
		let block_coord = TileCoord3::new(coord.get_x().shr(8), coord.get_y().shr(8), coord.get_z());

		// Get the block using the block coordinate
		let block_option = self.block_index.get_block(&block_coord);
		if block_option.is_none() {
			return create_error!("block <{block_coord:#?}> for tile <{coord:#?}> does not exist");
		}

		// Get the block and its bounding box
		let block: BlockDefinition = *block_option.unwrap();
		let bbox = block.get_global_bbox();

		// Calculate tile coordinates within the block
		let tile_coord: TileCoord2 = coord.as_coord2();

		// Check if the tile is within the block definition
		if !bbox.contains(&tile_coord) {
			return create_error!("tile {coord:?} outside block definition");
		}

		// Get the tile ID
		let tile_id = bbox.get_tile_index(&tile_coord);

		// Retrieve the tile index from cache or read from the reader
		let tile_index: Arc<TileIndex> = self.get_block_tile_index_cached(&block).await;
		let tile_range: &ByteRange = tile_index.get(tile_id);

		// Return None if the tile range has zero length
		if tile_range.length == 0 {
			return create_error!("tile_range.length == 0");
		}

		// Read the tile data from the reader
		self.reader.read_range(tile_range).await
	}

	async fn get_bbox_tile_stream<'a>(&'a mut self, bbox: &'a TileBBox) -> TileStream<'a> {
		const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
		const MAX_CHUNK_GAP: u64 = 32 * 1024;

		let mut outer_bbox: TileBBox = *bbox;

		let swap_xy: bool = self.get_parameters().unwrap().get_swap_xy();
		let flip_y: bool = self.get_parameters().unwrap().get_flip_y();

		if swap_xy {
			outer_bbox.swap_xy();
		};

		if flip_y {
			outer_bbox.flip_y();
		};

		let mut block_coords: TileBBox = outer_bbox;
		block_coords.scale_down(256);
		let block_coords: Vec<TileCoord3> = block_coords.iter_coords().collect();

		let self_mutex = Arc::new(Mutex::new(self));

		let chunks: Vec<Vec<Vec<(TileCoord3, ByteRange)>>> = futures_util::stream::iter(block_coords)
			.then(|block_coord: TileCoord3| {
				let self_mutex = self_mutex.clone();
				async move {
					let mut myself = self_mutex.lock().await;

					// Get the block using the block coordinate
					let block_option = myself.block_index.get_block(&block_coord);
					if block_option.is_none() {
						panic!("block <{block_coord:#?}> does not exist");
					}

					// Get the block
					let block: BlockDefinition = *block_option.unwrap();
					trace!("block {block:?}");

					// Get the bounding box of all tiles defined in this block
					let tiles_bbox_block = block.get_global_bbox();
					trace!("tiles_bbox_block {tiles_bbox_block:?}");

					// Get the bounding box of all tiles defined in this block
					let mut tiles_bbox_used = outer_bbox;
					tiles_bbox_used.intersect_bbox(tiles_bbox_block);
					trace!("tiles_bbox_used {tiles_bbox_used:?}");

					assert_eq!(outer_bbox.get_level(), tiles_bbox_block.get_level());
					assert_eq!(outer_bbox.get_level(), tiles_bbox_used.get_level());

					// Get the tile index of this block
					let tile_index: Arc<TileIndex> = myself.get_block_tile_index_cached(&block).await;
					trace!("tile_index {tile_index:?}");

					//let tile_range: &ByteRange = tile_index.get(tile_id);
					let mut tile_ranges: Vec<(TileCoord3, ByteRange)> = tile_index
						.iter()
						.enumerate()
						.map(|(index, range)| (tiles_bbox_block.get_coord3_by_index(index as u32), *range))
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
						.map(|(mut coord, range)| {
							let start = range.offset - offset;
							let end = start + range.length;
							let tile_range = (start as usize)..(end as usize);

							let blob = Blob::from(big_blob.get_range(tile_range));

							if flip_y {
								coord.flip_y();
							}

							if swap_xy {
								coord.swap_xy();
							}

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
	fn get_name(&self) -> Result<&str> {
		Ok(self.reader.get_name())
	}

	// deep probe of container meta
	#[cfg(feature = "full")]
	async fn probe_container(&mut self, print: PrettyPrint) -> Result<()> {
		print.add_key_value(&"meta size", &self.meta.len()).await;
		print.add_key_value(&"block count", &self.block_index.len()).await;

		let mut index_size = 0;
		let mut tiles_size = 0;

		for block in self.block_index.iter() {
			index_size += block.get_index_range().length;
			tiles_size += block.get_tiles_range().length;
		}

		print.add_key_value(&"sum of block index sizes", &index_size).await;
		print.add_key_value(&"sum of block tiles sizes", &tiles_size).await;

		Ok(())
	}
}

// Implement Debug for TileReader
impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:VersaTiles")
			.field("parameters", &self.get_parameters().unwrap())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	#[cfg(feature = "full")]
	use super::TileReader;
	#[cfg(feature = "full")]
	use crate::{
		containers::{tests::make_test_file, TileReaderTrait},
		shared::{Compression, Result, TileCoord3, TileFormat},
	};
	#[cfg(feature = "full")]
	use tokio;

	// Test tile fetching
	#[tokio::test]
	#[cfg(feature = "full")]
	async fn test_reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::PBF, Compression::Gzip, 8, "versatiles").await?;

		let mut reader = TileReader::new(temp_file.to_str().unwrap()).await?;

		let meta = reader.get_meta().await?;
		assert_eq!(meta.to_string(), "dummy meta data");

		let tile_data = reader.get_tile_data(&TileCoord3::new(2, 3, 4)).await?;
		assert_eq!(tile_data.len(), 77);

		Ok(())
	}
}
