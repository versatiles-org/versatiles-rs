// Import necessary modules and traits
use super::{new_data_reader, types::*, DataReaderTrait};
use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	shared::{
		Blob, DataConverter, ProgressBar, Result, StatusImagePyramide, TileCoord2, TileCoord3, TileReaderParameters,
	},
};
use async_trait::async_trait;
use itertools::Itertools;
use log::debug;
use std::{collections::HashMap, fmt::Debug, ops::Shr, path::Path};

// Define the TileReader struct
pub struct TileReader {
	meta: Blob,
	reader: Box<dyn DataReaderTrait>,
	parameters: TileReaderParameters,
	block_index: BlockIndex,
	tile_index_cache: HashMap<TileCoord3, TileIndex>,
}

// Implement methods for the TileReader struct
impl TileReader {
	// Create a new TileReader from a given data reader
	pub async fn from_src(mut reader: Box<dyn DataReaderTrait>) -> Result<TileReader> {
		let header = FileHeader::from_reader(&mut reader).await?;

		let meta = if header.meta_range.length > 0 {
			DataConverter::new_decompressor(&header.compression).run(reader.read_range(&header.meta_range).await?)?
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
	async fn get_tile_data(&mut self, coord_in: &TileCoord3) -> Option<Blob> {
		let mut coord: TileCoord3 = *coord_in;

		if self.get_parameters().unwrap().get_swap_xy() {
			coord.swap_xy();
		};

		if self.get_parameters().unwrap().get_flip_y() {
			coord.flip_y();
		};

		// Calculate block coordinate
		let block_coord = TileCoord3 {
			x: coord.x.shr(8),
			y: coord.y.shr(8),
			z: coord.z,
		};

		// Get the block using the block coordinate
		let block_option = self.block_index.get_block(&block_coord);
		if block_option.is_none() {
			log::debug!("block <{block_coord:#?}> for tile <{coord:#?}> does not exist");
			return None;
		}

		// Get the block and its bounding box
		let block = block_option.unwrap();
		let bbox = block.get_bbox();

		// Calculate tile coordinates within the block
		let tile_x = coord.x - block_coord.x * 256;
		let tile_y = coord.y - block_coord.y * 256;

		// Check if the tile is within the block definition
		if !bbox.contains(&TileCoord2::new(tile_x, tile_y)) {
			log::debug!("tile {coord:?} outside block definition");
			return None;
		}

		// Get the tile ID
		let tile_id = bbox.get_tile_index(&TileCoord2::new(tile_x, tile_y));

		// Retrieve the tile index from cache or read from the reader
		let tile_index_option = self.tile_index_cache.get(&block_coord);
		let tile_range: ByteRange;

		if let Some(tile_index) = tile_index_option {
			tile_range = *tile_index.get(tile_id);
		} else {
			let blob = self.reader.read_range(block.get_index_range()).await.unwrap();
			let mut tile_index = TileIndex::from_brotli_blob(blob);
			tile_index.add_offset(block.get_tiles_range().offset);

			self.tile_index_cache.insert(block_coord, tile_index);

			let tile_index_option = self.tile_index_cache.get(&block_coord);
			tile_range = *tile_index_option.unwrap().get(tile_id);
		}

		// Return None if the tile range has zero length
		if tile_range.length == 0 {
			return None;
		}

		// Read the tile data from the reader
		Some(self.reader.read_range(&tile_range).await.unwrap())
	}

	// Get the name of the reader
	fn get_name(&self) -> Result<&str> {
		Ok(self.reader.get_name())
	}

	// Perform a deep verification of the TileReader
	async fn deep_verify(&mut self, output_folder: &Path) -> Result<()> {
		let block_count = self.block_index.len() as u64;

		debug!("number of blocks: {}", block_count);

		let mut progress = ProgressBar::new("deep verify", self.block_index.get_bbox_pyramid().count_tiles());

		let blocks = self
			.block_index
			.iter()
			.sorted_by_cached_key(|block| block.get_sort_index());

		let mut status_images = StatusImagePyramide::new();

		for block in blocks {
			let bbox = block.get_bbox();
			let tiles_count = bbox.count_tiles();

			let blob = self.reader.read_range(block.get_index_range()).await?;
			let tile_index = TileIndex::from_brotli_blob(blob);
			assert_eq!(tile_index.len(), tiles_count as usize, "tile count are not the same");

			let status_image = status_images.get_level(block.get_z());

			let x_offset = block.get_x() * 256;
			let y_offset = block.get_y() * 256;

			for (index, byterange) in tile_index.iter().enumerate() {
				let coord = bbox.get_coord_by_index(index);
				status_image.set(coord.x + x_offset, coord.y + y_offset, byterange.length);
			}

			progress.inc(block.count_tiles());
		}
		progress.finish();

		status_images.save(&output_folder.join("tile_sizes.png"));

		Ok(())
	}
}

// Implement Debug for TileReader
impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:VersaTiles")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

// Unit tests
#[cfg(test)]
mod tests {
	use super::TileReader;
	use crate::{
		containers::{tests::make_test_file, TileReaderTrait},
		shared::{Compression, Result, TileFormat},
	};
	use assert_fs::TempDir;

	// Test deep verification
	#[tokio::test]
	async fn deep_verify() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let temp_file = make_test_file(TileFormat::PBF, Compression::Gzip, 8, "versatiles").await?;
		let mut reader = TileReader::new(temp_file.to_str().unwrap()).await?;
		reader.deep_verify(temp_dir.path()).await?;
		Ok(())
	}
}
