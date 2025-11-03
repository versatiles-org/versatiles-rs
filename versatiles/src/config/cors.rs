use serde::Deserialize;
use versatiles_derive::ConfigDoc;

#[derive(Default, Debug, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct Cors {
	/// Allowed origins for CORS requests
	/// Supports:
	/// - Globs at the start of the domain like `*.example.com`
	/// - Globs at the end of the domain like `example.*`
	/// - Regular expressions enclosed in slashes like `/domain\..*$/`
	#[serde(default)]
	#[config_demo(
		r#"
    - "https://example.org"
    - "*.example.net""#
	)]
	pub allowed_origins: Vec<String>,

	/// Optional duration for preflight cache in seconds
	/// Defaults to 86400 (1 day)
	#[serde(default)]
	#[config_demo("86400")]
	pub max_age_seconds: Option<u64>,
}
