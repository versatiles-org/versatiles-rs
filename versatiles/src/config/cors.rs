use serde::Deserialize;
use versatiles_derive::ConfigDoc;

#[derive(Default, Debug, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct Cors {
	/// Allowed origins
	/// Supports:
	/// - globs as first part of the domain like `*.example.com`
	/// - last part of the domain like `example.*`
	/// - regex if enclosed in slashes like `/domain\..*$/`
	#[serde(default)]
	#[config_demo(
		r#"
    - "https://example.org"
    - "*.example.net""#
	)]
	pub allowed_origins: Vec<String>,

	/// Optional preflight cache duration in seconds
	#[serde(default)]
	#[config_demo("86400")]
	pub max_age_seconds: Option<u64>,
}
