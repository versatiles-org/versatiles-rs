use anyhow::Result;
use serde::Deserialize;
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

/// Defines a static file source that is served under a specific URL prefix.
#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct StaticSourceConfig {
	#[config_demo("./frontend.tar")]
	/// Filesystem path or archive (e.g., .zip, .versatiles) containing static assets.
	pub path: UrlPath,
	#[config_demo("/")]
	/// Optional URL prefix under which the static files will be available. Example: "/" or "/assets".
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
