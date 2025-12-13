use crate::napi_result;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use versatiles_core::{
	TileCompression as RustTileCompression, TileCoord as RustTileCoord, TilesReaderParameters as RustReaderParameters,
};

/// Tile coordinate with zoom level (z), column (x), and row (y)
#[napi]
pub struct TileCoord {
	inner: RustTileCoord,
}

#[napi]
impl TileCoord {
	/// Create a new TileCoord
	#[napi(constructor)]
	pub fn new(z: u32, x: u32, y: u32) -> Result<Self> {
		let inner = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		Ok(Self { inner })
	}

	/// Create a TileCoord from geographic coordinates
	#[napi(factory)]
	pub fn from_geo(lon: f64, lat: f64, z: u32) -> Result<Self> {
		let inner = napi_result!(RustTileCoord::from_geo(lon, lat, z as u8))?;
		Ok(Self { inner })
	}

	/// Convert to geographic coordinates [longitude, latitude]
	#[napi]
	pub fn to_geo(&self) -> Vec<f64> {
		let [lon, lat] = self.inner.as_geo();
		vec![lon, lat]
	}

	/// Get the geographic bounding box [west, south, east, north]
	#[napi]
	pub fn to_geo_bbox(&self) -> Vec<f64> {
		let bbox = self.inner.to_geo_bbox();
		bbox.as_array().to_vec()
	}

	/// Get the zoom level
	#[napi(getter)]
	pub fn z(&self) -> u32 {
		self.inner.level as u32
	}

	/// Get the column (x)
	#[napi(getter)]
	pub fn x(&self) -> u32 {
		self.inner.x
	}

	/// Get the row (y)
	#[napi(getter)]
	pub fn y(&self) -> u32 {
		self.inner.y
	}

	/// Get JSON representation
	#[napi]
	pub fn to_json(&self) -> String {
		self.inner.as_json()
	}
}

/// Options for tile conversion
#[napi(object)]
pub struct ConvertOptions {
	/// Minimum zoom level to include
	pub min_zoom: Option<u8>,
	/// Maximum zoom level to include
	pub max_zoom: Option<u8>,
	/// Bounding box [west, south, east, north]
	pub bbox: Option<Vec<f64>>,
	/// Border around bbox in tiles
	pub bbox_border: Option<u32>,
	/// Compression: "gzip", "brotli", or "uncompressed"
	pub compress: Option<String>,
	/// Flip tiles vertically
	pub flip_y: Option<bool>,
	/// Swap x and y coordinates
	pub swap_xy: Option<bool>,
}

/// Options for tile server
#[napi(object)]
pub struct ServerOptions {
	/// IP address to bind (default: "0.0.0.0")
	pub ip: Option<String>,
	/// Port to listen on (default: 8080)
	pub port: Option<u32>,
	/// Use minimal recompression for better performance
	pub minimal_recompression: Option<bool>,
}

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

impl From<&RustReaderParameters> for ReaderParameters {
	fn from(params: &RustReaderParameters) -> Self {
		Self {
			tile_format: format!("{:?}", params.tile_format).to_lowercase(),
			tile_compression: format!("{:?}", params.tile_compression).to_lowercase(),
			min_zoom: params.bbox_pyramid.get_level_min().unwrap_or(0),
			max_zoom: params.bbox_pyramid.get_level_max().unwrap_or(0),
		}
	}
}

/// Probe result with container information
#[napi(object)]
pub struct ProbeResult {
	/// Source name or path
	pub source_name: String,
	/// Container type (e.g., "mbtiles", "versatiles")
	pub container_name: String,
	/// TileJSON metadata as JSON string
	pub tile_json: String,
	/// Reader parameters
	pub parameters: ReaderParameters,
}

/// Helper to parse compression string
pub fn parse_compression(s: &str) -> Option<RustTileCompression> {
	match s.to_lowercase().as_str() {
		"gzip" => Some(RustTileCompression::Gzip),
		"brotli" => Some(RustTileCompression::Brotli),
		"uncompressed" | "none" => Some(RustTileCompression::Uncompressed),
		_ => None,
	}
}
