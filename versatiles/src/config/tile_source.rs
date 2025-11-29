//! Configuration for tile data sources served by the VersaTiles HTTP server.
//!
//! Each entry in the `tiles` section of the main configuration file defines
//! one `TileSourceConfig`. These entries specify the name under which tiles
//! are served and the path or URL to their corresponding data source.
//!
//! # Example YAML
//! ```yaml
//! tiles:
//!   - ["osm", "osm.versatiles"]
//!   - ["berlin", "https://example.org/tileset.mbtiles"]
//! ```
//!
//! The server will make these tiles available under:
//! - `/tiles/osm/{z}/{x}/{y}`
//! - `/tiles/berlin/{z}/{x}/{y}`
use anyhow::Result;
use serde::Deserialize;
use std::fmt::Debug;
use versatiles_container::{DataLocation, DataSource};
use versatiles_derive::{ConfigDoc, context};

/// Configuration entry for a single tile data source.
///
/// Each `TileSourceConfig` entry maps a tile set name to a local or remote
/// path that provides the tile data.
///
/// Relative paths are resolved against the configuration file’s directory
/// by [`TileSourceConfig::resolve_paths`].
#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct TileSourceConfig {
	/// Optional name identifier for this tile source
	/// Tiles will be available under `/tiles/{name}/...`
	/// Defaults to the basename (e.g., "osm" for "osm.versatiles")
	#[config_demo("osm")]
	pub name: Option<String>,

	/// Path or URL to the tile data source
	/// Can be a local file or remote URL.
	#[config_demo("osm.versatiles")]
	pub path: DataSource,
}

impl TileSourceConfig {
	/// Resolve the `path` of this tile source relative to a given base directory or URL.
	///
	/// This is typically called by the configuration loader to ensure relative
	/// paths in YAML configs are interpreted correctly.
	///
	/// # Errors
	/// Returns an error if path resolution fails (e.g., invalid URL format).
	#[context("resolving tile source paths relative to base path '{}'", base_path)]
	pub fn resolve_paths(&mut self, base_path: &DataLocation) -> Result<()> {
		self.path.resolve(base_path)
	}
}

/// Custom deserializer that supports both shorthand array and explicit mapping syntax.
///
/// Example accepted YAML forms:
/// ```yaml
/// tiles:
///   - ["osm", "osm.versatiles"]
///   - name: "berlin"
///     path: "berlin.mbtiles"
/// ```
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
		}

		let helper = TileSourceConfigHelper::deserialize(deserializer)?;
		Ok(TileSourceConfig {
			name: helper.name,
			path: DataSource::parse(&helper.path).map_err(|e| serde::de::Error::custom(e.to_string()))?,
		})
	}
}

#[cfg(test)]
impl From<(&str, &str)> for TileSourceConfig {
	fn from((name, path): (&str, &str)) -> Self {
		Self {
			name: Some(name.to_string()),
			path: DataSource::try_from(path).unwrap(),
		}
	}
}
