use serde::Deserialize;
use versatiles_derive::ConfigDoc;

#[derive(Default, Debug, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct Cors {
	/// Allowed origins (supports globs in your app logic)
	#[serde(default)]
	#[config_demo(
		r#"
    - "https://example.org"
    - "*.example.net""#
	)]
	pub allowed_origins: Vec<String>,

	/// Preflight cache duration in seconds
	#[serde(default)]
	#[config_demo("86400")]
	pub max_age_seconds: Option<u64>,
}
