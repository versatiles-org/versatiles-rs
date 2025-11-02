use anyhow::Result;
use serde::Deserialize;
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct StaticSourceConfig {
	pub path: UrlPath,
	pub url_prefix: Option<String>,
}

impl StaticSourceConfig {
	pub fn resolve_paths(&mut self, base_path: &UrlPath) -> Result<()> {
		self.path.resolve(base_path)
	}
}

impl<'de> Deserialize<'de> for StaticSourceConfig {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct StaticSourceConfigHelper {
			pub path: String,
			pub url_prefix: Option<String>,
		}

		let helper = StaticSourceConfigHelper::deserialize(deserializer)?;
		Ok(StaticSourceConfig {
			path: UrlPath::from(helper.path),
			url_prefix: helper.url_prefix,
		})
	}
}
