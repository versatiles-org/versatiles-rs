use anyhow::Result;
use serde::Deserialize;
use std::fmt::Debug;
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct TileSourceConfig {
	/// Optional name identifier for this tile source
	/// Tiles will be available under `/tiles/{name}/...`
	/// Defaults to the last part of the path (e.g., "osm" for "osm.versatiles")
	#[config_demo("osm")]
	pub name: Option<String>,

	/// Path or URL to the tile data source
	/// Can be a local file or remote URL.
	#[config_demo("osm.versatiles")]
	pub path: UrlPath,
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
		}

		let helper = TileSourceConfigHelper::deserialize(deserializer)?;
		Ok(TileSourceConfig {
			name: helper.name,
			path: UrlPath::from(helper.path),
		})
	}
}

#[cfg(test)]
impl From<(&str, &str)> for TileSourceConfig {
	fn from((name, path): (&str, &str)) -> Self {
		Self {
			name: Some(name.to_string()),
			path: UrlPath::from(path),
		}
	}
}
