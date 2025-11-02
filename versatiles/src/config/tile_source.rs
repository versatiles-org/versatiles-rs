use anyhow::Result;
use serde::Deserialize;
use std::fmt::Debug;
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

/// Describes a tile source that the server can serve, including file path and optional coordinate transformations.
#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct TileSourceConfig {
	/// Optional identifier used to reference this tile source.
	#[config_demo("osm")]
	pub name: Option<String>,
	/// Path or URL to the tile data. Can point to a local file or remote source.
	#[config_demo("osm.versatiles")]
	pub path: UrlPath,
	/// If true, flips the Y-axis of tile coordinates (useful for TMS vs XYZ layouts).
	#[config_demo("false")]
	pub flip_y: Option<bool>,
	/// If true, swaps the X and Y coordinates (rare, but needed for some projections).
	#[config_demo("false")]
	pub swap_xy: Option<bool>,
	/// Overrides the compression format for this tile source (e.g., "gzip", "brotli").
	#[config_demo("brotli")]
	pub override_compression: Option<String>,
}

impl TileSourceConfig {
	pub fn resolve_paths(&mut self, base_path: &UrlPath) -> Result<()> {
		self.path.resolve(base_path)
	}
}

impl<'de> Deserialize<'de> for TileSourceConfig {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct TileSourceConfigHelper {
			pub name: Option<String>,
			pub path: String,
			pub flip_y: Option<bool>,
			pub swap_xy: Option<bool>,
			pub override_compression: Option<String>,
		}

		let helper = TileSourceConfigHelper::deserialize(deserializer)?;
		Ok(TileSourceConfig {
			name: helper.name,
			path: UrlPath::from(helper.path),
			flip_y: helper.flip_y,
			swap_xy: helper.swap_xy,
			override_compression: helper.override_compression,
		})
	}
}
