use crate::opencloudtiles::lib::*;
use std::{fmt::Debug, path::Path};

pub type TileConverterBox = Box<dyn TileConverterTrait>;
pub type TileReaderBox = Box<dyn TileReaderTrait>;

#[allow(clippy::new_ret_no_self)]
pub trait TileConverterTrait {
	fn new(filename: &Path, config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	fn convert_from(&mut self, reader: &mut TileReaderBox);
}

#[allow(clippy::new_ret_no_self)]
pub trait TileReaderTrait: Debug + Send + Sync {
	fn new(path: &str) -> TileReaderBox
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

	/// always uncompressed
	fn get_meta(&self) -> Blob;

	/// always compressed with get_tile_precompression and formatted with get_tile_format
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob>;

	/// always compressed with get_tile_precompression and formatted with get_tile_format
	fn get_bbox_tile_vec(&self, zoom: u64, bbox: &TileBBox) -> Vec<(TileCoord2, Blob)> {
		bbox
			.iter_coords()
			.filter_map(move |coord: TileCoord2| -> Option<(TileCoord2, Blob)> {
				self
					.get_tile_data(&coord.add_zoom(zoom))
					.map(|blob| (coord, blob))
			})
			.collect()
	}

	fn deep_verify(&self);
}
