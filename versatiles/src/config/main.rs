//! High-level server configuration loader for VersaTiles.
//!
//! This module defines the top-level [`Config`] struct and helpers to parse YAML from
//! strings, readers, or file paths. It also resolves relative paths in `static` and
//! `tiles` sources relative to the config file location.
//!
//! ## YAML shape
//!
//! ```yaml
//! # Optional HTTP server configuration
//! server:
//!   ip: 0.0.0.0
//!   port: 8080
//!   minimal_recompression: false   # optional
//!   disable_api: false             # optional
//!
//! # Optional Cross-Origin Resource Sharing (CORS) settings
//! cors:
//!   allowed_origins:
//!     - https://example.org
//!     - "*.example.net"
//!   max_age_seconds: 86400         # optional
//!
//! # Optional extra HTTP response headers
//! extra_response_headers:
//!   Cache-Control: "public, max-age=86400, immutable"
//!   CDN-Cache-Control: "max-age=604800"
//!
//! # Optional list of static content sources
//! static:
//!   - ["/", "./frontend.tar"]
//!   - ["/assets", "./public"]
//!
//! # Optional list of tile sources
//! tiles:
//!   - ["osm", "osm.versatiles"]
//!   - ["berlin", "berlin.mbtiles"]
//! ```
//!
//! ## Basic usage
//! Reading from a file and resolving relative paths:
//! ```no_run
//! use std::path::Path;
//! use versatiles::Config;
//! let cfg = Config::from_path(Path::new("server.yml")).expect("config");
//! ```
//! Parsing from a string (e.g., tests):
//! ```no_run
//! use versatiles::Config;
//! let cfg = Config::from_string("tiles: [[\"osm\", \"osm.versatiles\"]]").unwrap();
//! ```
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
use versatiles_derive::context;

/// Top-level server configuration.
///
/// All sections are **optional** and default to empty values. Missing sections are treated
/// as if present with defaults (e.g., no `static` sources, no `tiles`, empty `extra_response_headers`).
///
/// The `static` and `tiles` arrays accept pairs where the first element is a mount key and the
/// second element is a path/URL. Relative paths are resolved against the config file directory
/// via [`Config::from_path`], then further normalized with [`Config::resolve_paths`].
///
/// See the module-level docs for a full YAML example.
#[derive(Default, Debug, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct Config {
	/// Optional HTTP server configuration
	#[serde(default)]
	pub server: ServerConfig,

	/// Optional Cross-Origin Resource Sharing (CORS) settings
	#[serde(default)]
	pub cors: Cors,

	/// Optional extra HTTP response headers to add to every response
	/// For example, cache control or timing headers
	#[serde(default)]
	#[config_demo(
		r#"
  Cache-Control: public, max-age=86400, immutable
  CDN-Cache-Control: max-age=604800"#
	)]
	pub extra_response_headers: HashMap<String, String>,

	/// Optional list of static content sources
	#[serde(default, rename = "static")]
	pub static_sources: Vec<StaticSourceConfig>,

	/// Optional list of tile sources
	#[serde(default, rename = "tiles")]
	pub tile_sources: Vec<TileSourceConfig>,
}

impl Config {
	/// Parse a YAML config from any `Read` implementor.
	///
	/// Useful when loading from in-memory buffers or network streams.
	/// Errors include a contextual message with the operation being performed.
	#[context("parsing config from reader (YAML)")]
	pub fn from_reader<R: Read>(reader: R) -> Result<Self> {
		Ok(serde_yaml_ng::from_reader(reader)?)
	}

	/// Parse a YAML config from a string slice.
	///
	/// Convenience for tests and simple setups.
	#[context("parsing config from string (YAML)")]
	pub fn from_string(text: &str) -> Result<Self> {
		Ok(serde_yaml_ng::from_str(text)?)
	}

	/// Parse from a file path and resolve relative paths for all sources.
	///
	/// Merge strategy (if you compose configs elsewhere before calling this):
	/// - Arrays (`static`, `tiles`) are **appended** in include order.
	/// - Maps (`extra_response_headers`) are **shallow-merged**; later files win on key conflicts.
	/// - `server` and `cors` fields are **shallow-overridden** field-by-field by later configs.
	#[context("reading config file '{}'", path.display())]
	pub fn from_path(path: &Path) -> Result<Self> {
		let file = File::open(path)?;
		let mut cfg = Config::from_reader(BufReader::new(file))?;

		// Sanity checks
		cfg.resolve_paths(&UrlPath::from(path.parent().unwrap()))?;
		Ok(cfg)
	}

	/// Resolve all relative paths in `static_sources` and `tile_sources` against `base`.
	///
	/// `base` should be the directory containing the YAML file (or an equivalent URL base).
	/// Paths are left unchanged if they are already absolute; URLs are left unchanged.
	#[context("resolving relative paths for {} static + {} tile sources", self.static_sources.len(), self.tile_sources.len())]
	pub fn resolve_paths(&mut self, base: &UrlPath) -> Result<()> {
		for static_source in &mut self.static_sources {
			static_source.resolve_paths(base)?;
		}

		for tile_source in &mut self.tile_sources {
			tile_source.resolve_paths(base)?;
		}

		Ok(())
	}

	/// Render Markdown help: the prose from `help.md` followed by a fenced YAML demo block.
	///
	/// This is consumed by UIs or `--help` outputs that want embedded examples.
	pub fn help_md() -> String {
		[
			include_str!("help.md").trim(),
			"\n```yaml",
			Self::demo_yaml_with_indent(0).trim(),
			"```",
		]
		.join("\n")
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
		assert_eq!(
			cfg.unwrap_err().chain().map(|e| e.to_string()).collect::<Vec<_>>(),
			vec![
				"parsing config from string (YAML)",
				"server: unknown field `pi`, expected one of `ip`, `port`, `minimal_recompression`, `disable_api` at line 2 column 3"
			]
		);
	}

	#[test]
	fn parse_demo_config() {
		let yaml = Config::demo_yaml_with_indent(0);
		let cfg = Config::from_string(&yaml).unwrap();
		assert_eq!(
			cfg,
			Config {
				server: ServerConfig {
					ip: Some("0.0.0.0".to_string()),
					port: Some(8080,),
					minimal_recompression: Some(false,),
					disable_api: Some(false,),
				},
				cors: Cors {
					allowed_origins: vec!["https://example.org".to_string(), "*.example.net".to_string()],
					max_age_seconds: Some(86400),
				},
				extra_response_headers: [
					("CDN-Cache-Control", "max-age=604800"),
					("Cache-Control", "public, max-age=86400, immutable"),
				]
				.iter()
				.map(|(a, b)| (a.to_string(), b.to_string()))
				.collect::<HashMap<String, String>>(),
				static_sources: vec![StaticSourceConfig::from(("/", "./frontend.tar")),],
				tile_sources: vec![TileSourceConfig::from(("osm", "osm.versatiles")),],
			}
		)
	}
}
