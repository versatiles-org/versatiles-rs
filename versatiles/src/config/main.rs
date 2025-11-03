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
	/// HTTP server configuration options (port, IP, etc.)
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
  CDN-Cache-Control: max-age=604800
"#
	)]
	pub extra_response_headers: HashMap<String, String>,

	/// List of static sources mounted to specific URL prefixes
	#[serde(default, rename = "static")]
	pub static_sources: Vec<StaticSourceConfig>,

	/// List of tile sources that the server can serve
	#[serde(default, rename = "tiles")]
	pub tile_sources: Vec<TileSourceConfig>,
}

impl Config {
	pub fn from_reader<R: Read>(reader: R) -> Result<Self> {
		let cfg = serde_yaml_ng::from_reader(reader)?;
		Ok(cfg)
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
	fn parse_example_config() -> Result<()> {
		// Adjust the path to wherever your test YAML lives (matches your workspace layout).
		let path = Path::new("../testdata/config1.yml");
		let cfg = Config::from_path(path)?;

		assert_eq!(
			cfg,
			Config {
				server: ServerConfig {
					ip: Some("127.0.0.1".parse()?),
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
					StaticSourceConfig {
						path: UrlPath::from("../testdata/static.tar.br"),
						url_prefix: Some("/".to_string())
					},
					StaticSourceConfig {
						path: UrlPath::from("../testdata/static.tar.gz"),
						url_prefix: Some("/whynot/".to_string())
					},
					StaticSourceConfig {
						path: UrlPath::from("../testdata"),
						url_prefix: Some("/assets".to_string())
					}
				],
				tile_sources: vec![
					TileSourceConfig {
						name: Some("osm".to_string()),
						path: UrlPath::from("https://download.versatiles.org/osm.versatiles"),
						flip_y: Some(false),
						swap_xy: Some(false),
						override_compression: Some("brotli".to_string())
					},
					TileSourceConfig {
						name: Some("berlin".to_string()),
						path: UrlPath::from("../testdata/berlin.mbtiles"),
						flip_y: None,
						swap_xy: None,
						override_compression: None
					},
					TileSourceConfig {
						name: Some("pipeline".to_string()),
						path: UrlPath::from("../testdata/berlin.vpl"),
						flip_y: Some(true),
						swap_xy: Some(true),
						override_compression: None
					}
				]
			}
		);
		Ok(())
	}
}
