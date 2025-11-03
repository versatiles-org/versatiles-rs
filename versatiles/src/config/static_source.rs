use anyhow::Result;
use serde::Deserialize;
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct StaticSourceConfig {
	#[config_demo("./frontend.tar")]
	/// Path to static files or archive (e.g., .tar.gz) containing assets
	pub path: UrlPath,

	#[config_demo("/")]
	/// Optional URL prefix where static files will be served
	/// Defaults to root ("/")
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

#[cfg(test)]
impl From<(&str, &str)> for StaticSourceConfig {
	fn from((url_prefix, path): (&str, &str)) -> Self {
		Self {
			path: UrlPath::from(path),
			url_prefix: Some(url_prefix.to_string()),
		}
	}
}
