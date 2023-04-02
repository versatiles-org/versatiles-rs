use async_trait::async_trait;
use std::{fmt::Debug, path::Path};
use versatiles_shared::*;

pub type TileConverterBox = Box<dyn TileConverterTrait>;
pub type TileReaderBox = Box<dyn TileReaderTrait>;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TileConverterTrait {
	fn new(filename: &Path, config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	async fn convert_from(&mut self, reader: &mut TileReaderBox);
}

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait TileReaderTrait: Debug + Send + Sync {
	async fn new(path: &str) -> Result<TileReaderBox, Error>
	where
		Self: Sized;
	fn get_name(&self) -> &str;
	fn get_parameters(&self) -> &TileReaderParameters;
	fn get_parameters_mut(&mut self) -> &mut TileReaderParameters;
	fn get_tile_format(&self) -> &TileFormat {
		self.get_parameters().get_tile_format()
	}
	fn get_tile_precompression(&self) -> &Precompression {
		self.get_parameters().get_tile_precompression()
	}
	fn get_container_name(&self) -> &str;

	/// always uncompressed
	async fn get_meta(&self) -> Blob;

	/// always compressed with get_tile_precompression and formatted with get_tile_format
	async fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob>;

	/// always compressed with get_tile_precompression and formatted with get_tile_format
	async fn get_bbox_tile_vec(&self, zoom: u8, bbox: &TileBBox) -> Vec<(TileCoord2, Blob)> {
		let mut vec: Vec<(TileCoord2, Blob)> = Vec::new();
		for coord in bbox.iter_coords() {
			let option = self.get_tile_data(&coord.with_zoom(zoom)).await;
			if let Some(blob) = option {
				vec.push((coord, blob));
			}
		}
		return vec;
	}

	async fn deep_verify(&self);
}
