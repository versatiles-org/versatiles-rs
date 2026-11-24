//! Configuration for static file sources served by the VersaTiles HTTP server.
//!
//! Each entry in the `static` section of the main configuration file defines
//! one `StaticSourceConfig`. These describe where the static assets are located
//! and under which URL prefix they should be served.
//!
//! # Example YAML
//! ```yaml
//! static:
//!   - ["/", "./frontend.tar"]
//!   - ["/assets", "./public"]
//! ```
//!
//! The server will serve:
//! - the first entry at the root `/` from a tar archive named `frontend.tar`
//! - the second entry under `/assets` from the folder `public/`

use anyhow::Result;
use serde::Deserialize;
use versatiles_container::DataLocation;
use versatiles_derive::{ConfigDoc, context};

use super::PathPattern;

/// Default `Cache-Control` for static files: check with the server every time.
///
/// Static files are served under fixed URLs, so any lifetime is how long a
/// browser keeps using a file after it changed. Under the four weeks tiles
/// get, a frontend update reached the HTML at once but its scripts only weeks
/// later (versatiles-org/versatiles-frontend#96). With `no-cache` a browser
/// asks every time, and the `ETag` turns an unchanged file into an empty `304`.
pub const DEFAULT_STATIC_CACHE_CONTROL: &str = "no-cache";

/// `Cache-Control` for the files an `immutable` pattern matches: one year.
///
/// Such a file's URL contains a version, so its content never changes under
/// that URL. A browser keeps it for a year without asking, and `immutable`
/// tells it not to ask on a reload either (#337).
pub const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// Configuration entry for serving static assets.
///
/// A `StaticSourceConfig` defines the local path or archive of static files
/// and the corresponding URL prefix under which they are made available.
///
/// This is used by the `StaticSources` subsystem to register handlers for
/// static file serving.
///
/// - `src` — Path to a directory or archive (`.tar`, `.tar.gz`, `.tar.br`, `.tar.zst`).
/// - `prefix` — Optional base URL prefix (defaults to `/` if `None`).
/// - `cache_control` — Optional `Cache-Control` header (defaults to
///   [`DEFAULT_STATIC_CACHE_CONTROL`] if `None`).
/// - `immutable` — Patterns of versioned paths, sent with
///   [`IMMUTABLE_CACHE_CONTROL`] instead.
///
/// Relative paths are resolved against the base path of the configuration file
/// by [`StaticSourceConfig::resolve_paths`].
#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct StaticSourceConfig {
	#[config_demo("./frontend.tar")]
	/// Path to static files, archive (e.g., .tar.gz) or directory containing assets
	pub src: DataLocation,

	#[config_demo("/")]
	/// Optional URL prefix where static files will be served
	/// Defaults to root ("/")
	pub prefix: Option<String>,

	#[config_demo("no-cache")]
	/// Optional `Cache-Control` header sent with these files
	/// Defaults to "no-cache": a browser checks every time, and an unchanged
	/// file costs an empty 304. Static URLs stay the same when the files
	/// change, so a longer value keeps old files in browsers after an update.
	/// `server.cache_control` does not apply to static files.
	pub cache_control: Option<String>,

	#[config_demo("[\"assets/lib/*@*/**\"]")]
	/// Optional list of path patterns for files whose path contains a version
	/// Matching files are sent with "public, max-age=31536000, immutable"
	/// instead of `cache_control`: their content never changes under that URL.
	/// A pattern is a path inside this source, without `prefix`: `*` matches
	/// within one segment, `**` across segments. Let a pattern match only
	/// versioned folders (`assets/lib/*@*/**`, never `assets/lib/**`): an
	/// outdated pattern then matches nothing, and files fall back to
	/// `cache_control` instead of staying in browsers for a year.
	pub immutable: Vec<String>,
}

impl StaticSourceConfig {
	/// The `Cache-Control` header these files are sent with.
	#[must_use]
	pub fn cache_control(&self) -> &str {
		self.cache_control.as_deref().unwrap_or(DEFAULT_STATIC_CACHE_CONTROL)
	}

	/// The `immutable` patterns, parsed.
	///
	/// # Errors
	/// Returns an error naming the first pattern that cannot be parsed.
	pub fn immutable_patterns(&self) -> Result<Vec<PathPattern>> {
		self.immutable.iter().map(|text| PathPattern::parse(text)).collect()
	}

	/// Resolve the `path` relative to a provided base directory or URL.
	///
	/// This allows relative paths in configuration files to be interpreted
	/// relative to the YAML file’s location. URLs remain unchanged.
	///
	/// # Errors
	/// Returns an error if path resolution fails (for example, invalid URLs).
	#[context("resolving static source paths relative to base path '{}'", base_path)]
	pub fn resolve_paths(&mut self, base_path: &DataLocation) -> Result<()> {
		self.src.resolve(base_path)
	}
}

/// Custom deserializer supporting both key-value pair arrays and explicit mapping forms.
///
/// Example accepted formats:
/// ```yaml
/// static:
///   - ["/", "./frontend.tar"]
///   - src: "./public"
///     prefix: "/assets"
/// ```
impl<'de> Deserialize<'de> for StaticSourceConfig {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct StaticSourceConfigHelper {
			pub src: String,
			pub prefix: Option<String>,
			pub cache_control: Option<String>,
			#[serde(default)]
			pub immutable: Vec<String>,
		}

		let helper = super::pair::pair_or_mapping(
			deserializer,
			|prefix, src| StaticSourceConfigHelper {
				src,
				prefix: Some(prefix),
				cache_control: None,
				immutable: Vec::new(),
			},
			"a [prefix, src] pair, or a mapping with `src` and optional `prefix`, `cache_control` and `immutable`",
		)?;
		let config = StaticSourceConfig {
			src: DataLocation::try_from(helper.src).map_err(serde::de::Error::custom)?,
			prefix: helper.prefix,
			cache_control: helper.cache_control,
			immutable: helper.immutable,
		};
		// A bad pattern fails when the config is loaded, not when the source is
		// mounted.
		config.immutable_patterns().map_err(serde::de::Error::custom)?;
		Ok(config)
	}
}

#[cfg(test)]
impl From<(&str, &str)> for StaticSourceConfig {
	fn from((prefix, src): (&str, &str)) -> Self {
		Self {
			src: DataLocation::try_from(src).unwrap(),
			prefix: Some(prefix.to_string()),
			cache_control: None,
			immutable: Vec::new(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(yaml: &str) -> StaticSourceConfig {
		serde_yaml_ng::from_str(yaml).unwrap()
	}

	#[test]
	fn cache_control_defaults_to_no_cache() {
		assert_eq!(parse("src: ./public").cache_control(), "no-cache");
		assert_eq!(parse(r#"["/", "./public"]"#).cache_control(), "no-cache");
	}

	#[test]
	fn cache_control_can_be_set_per_source() {
		let config = parse("src: ./public\ncache_control: public, max-age=60");
		assert_eq!(config.cache_control(), "public, max-age=60");
	}

	#[test]
	fn immutable_defaults_to_none() {
		assert_eq!(parse("src: ./public").immutable, Vec::<String>::new());
		assert_eq!(parse(r#"["/", "./public"]"#).immutable, Vec::<String>::new());
	}

	#[test]
	fn immutable_patterns_are_read() {
		let config = parse("src: ./public\nimmutable: ['assets/lib/*@*/**', 'fonts/*@*/**']");
		assert_eq!(config.immutable, ["assets/lib/*@*/**", "fonts/*@*/**"]);
		assert_eq!(config.immutable_patterns().unwrap().len(), 2);
	}

	#[test]
	fn a_bad_immutable_pattern_fails_when_the_config_is_loaded() {
		let error = serde_yaml_ng::from_str::<StaticSourceConfig>("src: ./public\nimmutable: ['/assets/**']")
			.unwrap_err()
			.to_string();
		assert!(error.contains("starts with '/'"), "{error}");
	}
}
