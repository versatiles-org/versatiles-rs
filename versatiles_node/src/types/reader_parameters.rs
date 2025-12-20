use napi_derive::napi;
use versatiles_core::TilesReaderParameters;

/// Tile reader parameters
#[napi(object)]
#[derive(Clone)]
pub struct ReaderParameters {
	/// Tile format (e.g., "png", "jpg", "mvt")
	pub tile_format: String,
	/// Tile compression (e.g., "gzip", "brotli", "uncompressed")
	pub tile_compression: String,
	/// Minimum zoom level available
	pub min_zoom: u8,
	/// Maximum zoom level available
	pub max_zoom: u8,
}

impl From<&TilesReaderParameters> for ReaderParameters {
	fn from(params: &TilesReaderParameters) -> Self {
		Self {
			tile_format: format!("{:?}", params.tile_format).to_lowercase(),
			tile_compression: format!("{:?}", params.tile_compression).to_lowercase(),
			min_zoom: params.bbox_pyramid.get_level_min().unwrap_or(0),
			max_zoom: params.bbox_pyramid.get_level_max().unwrap_or(0),
		}
	}
}
