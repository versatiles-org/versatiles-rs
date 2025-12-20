use super::ReaderParameters;
use napi_derive::napi;

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
