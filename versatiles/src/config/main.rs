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

#[derive(Default, Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
	#[serde(default)]
	pub server: ServerConfig,

	#[serde(default)]
	pub cors: Cors,

	/// Extra response headers added to every HTTP response.
	/// Case-insensitivity is a runtime concern; we store as given.
	#[serde(default)]
	pub extra_response_headers: HashMap<String, String>,

	/// Static mounts
	#[serde(default, rename = "static")]
	pub static_sources: Vec<StaticSourceConfig>,

	/// Tile sources
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

	#[test]
	fn parse_example_config() -> Result<()> {
		// Adjust the path to wherever your test YAML lives (matches your workspace layout).
		let path = Path::new("../testdata/config1.yml");
		let cfg = Config::from_path(path)?;

		assert_eq!(
			cfg,
			Config {
				server: ServerConfig {
					ip: Some("1.2.3.4".parse()?),
					port: Some(1234),
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
						path: UrlPath::from("/web/site.tar.br"),
						url_prefix: Some("/".to_string())
					},
					StaticSourceConfig {
						path: UrlPath::from("../testdata/assets/"),
						url_prefix: Some("/assets".to_string())
					}
				],
				tile_sources: vec![
					TileSourceConfig {
						name: Some("osm".to_string()),
						path: UrlPath::from("../testdata/https://download.versatiles.org/osm.versatiles"),
						flip_y: Some(false),
						swap_xy: Some(false),
						override_compression: Some("gzip".to_string())
					},
					TileSourceConfig {
						name: Some("berlin".to_string()),
						path: UrlPath::from("../testdata/../testdata/berlin.mbtiles"),
						flip_y: None,
						swap_xy: None,
						override_compression: None
					},
					TileSourceConfig {
						name: Some("planet".to_string()),
						path: UrlPath::from("/data/tiles/planet.tar.br"),
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
