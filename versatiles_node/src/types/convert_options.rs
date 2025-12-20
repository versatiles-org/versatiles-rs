use napi_derive::napi;

/// Options for tile conversion
#[napi(object)]
#[derive(Clone)]
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
