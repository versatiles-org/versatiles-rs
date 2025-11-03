use super::{Cors, ServerConfig, StaticSourceConfig, TileSourceConfig};
use anyhow::Result;
use serde::Deserialize;
use std::{
	collections::HashMap,
	fs::File,
	io::{BufReader, Read},
	path::Path,
};
use versatiles_container::UrlPath;
use versatiles_derive::ConfigDoc;

#[derive(Default, Debug, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct Config {
	/// HTTP server configuration
	#[serde(default)]
	pub server: ServerConfig,

	/// Cross-Origin Resource Sharing (CORS) settings
	#[serde(default)]
	pub cors: Cors,

	/// Extra response headers added to every HTTP response.
	/// For example, cache control headers or timing headers can be specified here.
	#[serde(default)]
	#[config_demo(
		r#"
  Cache-Control: public, max-age=86400, immutable
  CDN-Cache-Control: max-age=604800"#
	)]
	pub extra_response_headers: HashMap<String, String>,

	/// List of static sources
	#[serde(default, rename = "static")]
	pub static_sources: Vec<StaticSourceConfig>,

	/// List of tile sources
	#[serde(default, rename = "tiles")]
	pub tile_sources: Vec<TileSourceConfig>,
}

impl Config {
	pub fn from_reader<R: Read>(reader: R) -> Result<Self> {
		Ok(serde_yaml_ng::from_reader(reader)?)
	}

	pub fn from_string(text: &str) -> Result<Self> {
		Ok(serde_yaml_ng::from_str(text)?)
	}

	/// Parse from a file path and resolve `include.files` relative to that file.
	///
	/// Merge strategy:
	/// - Arrays (`statik`, `tiles`) are appended in include order.
	/// - Maps (`extra_response_headers`) are shallow-merged; later files win on key conflicts.
	/// - `server` and `cors` from the root remain unless an include provides those fields; if so, includes override field-by-field (shallow).
	pub fn from_path(path: &Path) -> Result<Self> {
		let file = File::open(path)?;
		let mut cfg = Config::from_reader(BufReader::new(file))?;

		// Sanity checks
		cfg.resolve_paths(&UrlPath::from(path.parent().unwrap()))?;
		Ok(cfg)
	}

	pub fn resolve_paths(&mut self, base: &UrlPath) -> Result<()> {
		for static_source in &mut self.static_sources {
			static_source.resolve_paths(base)?;
		}

		for tile_source in &mut self.tile_sources {
			tile_source.resolve_paths(base)?;
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;

	#[test]
	fn parse_example_config() {
		// Adjust the path to wherever your test YAML lives (matches your workspace layout).
		let path = Path::new("../testdata/config1.yml");
		let cfg = Config::from_path(path).unwrap();

		assert_eq!(
			cfg,
			Config {
				server: ServerConfig {
					ip: Some("127.0.0.1".parse().unwrap()),
					port: Some(51234),
					minimal_recompression: Some(true),
					disable_api: Some(true)
				},
				cors: Cors {
					allowed_origins: vec!["https://example.org".to_string(), "*.other-example.org".to_string()],
					max_age_seconds: Some(86400)
				},
				extra_response_headers: [
					("Timing-Allow-Origin", "*"),
					("CDN-Cache-Control", "max-age=604800"),
					("Cache-Control", "public, max-age=86400, immutable"),
					("Surrogate-Control", "max-age=604800")
				]
				.iter()
				.map(|(a, b)| (a.to_string(), b.to_string()))
				.collect::<HashMap<String, String>>(),
				static_sources: vec![
					StaticSourceConfig::from(("/", "../testdata/static.tar.br")),
					StaticSourceConfig::from(("/whynot/", "../testdata/static.tar.gz")),
					StaticSourceConfig::from(("/assets", "../testdata"))
				],
				tile_sources: vec![
					TileSourceConfig::from(("osm", "https://download.versatiles.org/osm.versatiles")),
					TileSourceConfig::from(("berlin", "../testdata/berlin.mbtiles")),
					TileSourceConfig::from(("pipeline", "../testdata/berlin.vpl"))
				]
			}
		);
	}

	#[test]
	fn parse_empty_config() {
		assert_eq!(Config::from_string("").unwrap(), Config::default());
	}

	#[test]
	fn parse_invalid_config() {
		let cfg = Config::from_string("server:\n  pi: 3.14.15.9");
	}

	#[test]
	fn parse_demo_config() {
		let yaml = Config::demo_yaml();
		let cfg = Config::from_string(&yaml).unwrap();
	}
}
