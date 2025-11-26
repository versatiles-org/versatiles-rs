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

/// Configuration entry for serving static assets.
///
/// A `StaticSourceConfig` defines the local path or archive of static files
/// and the corresponding URL prefix under which they are made available.
///
/// This is used by the `StaticSources` subsystem to register handlers for
/// static file serving.
///
/// - `path` — Path to a directory or archive (`.tar`, `.tar.gz`, `.tar.zst`).
/// - `url_prefix` — Optional base URL prefix (defaults to `/` if `None`).
///
/// Relative paths are resolved against the base path of the configuration file
/// by [`StaticSourceConfig::resolve_paths`].
#[derive(Debug, Clone, PartialEq, ConfigDoc)]
pub struct StaticSourceConfig {
	#[config_demo("./frontend.tar")]
	/// Path to static files or archive (e.g., .tar.gz) containing assets
	pub path: DataLocation,

	#[config_demo("/")]
	/// Optional URL prefix where static files will be served
	/// Defaults to root ("/")
	pub url_prefix: Option<String>,
}

impl StaticSourceConfig {
	/// Resolve the `path` relative to a provided base directory or URL.
	///
	/// This allows relative paths in configuration files to be interpreted
	/// relative to the YAML file’s location. URLs remain unchanged.
	///
	/// # Errors
	/// Returns an error if path resolution fails (for example, invalid URLs).
	#[context("resolving static source paths relative to base path '{}'", base_path)]
	pub fn resolve_paths(&mut self, base_path: &DataLocation) -> Result<()> {
		self.path.resolve(base_path)
	}
}

/// Custom deserializer supporting both key-value pair arrays and explicit mapping forms.
///
/// Example accepted formats:
/// ```yaml
/// static:
///   - ["/", "./frontend.tar"]
///   - path: "./public"
///     url_prefix: "/assets"
/// ```
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
			path: DataLocation::from(helper.path),
			url_prefix: helper.url_prefix,
		})
	}
}

#[cfg(test)]
impl From<(&str, &str)> for StaticSourceConfig {
	fn from((url_prefix, path): (&str, &str)) -> Self {
		Self {
			path: DataLocation::from(path),
			url_prefix: Some(url_prefix.to_string()),
		}
	}
}
