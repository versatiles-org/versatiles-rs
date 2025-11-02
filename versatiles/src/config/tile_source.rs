use anyhow::Result;
use serde::Deserialize;
use std::fmt::Debug;
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct TileSourceConfig {
	pub name: Option<String>,
	pub path: UrlPath,
	pub flip_y: Option<bool>,
	pub swap_xy: Option<bool>,
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
