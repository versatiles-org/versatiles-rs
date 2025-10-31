use serde::Deserialize;

#[derive(Default, Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Cors {
	/// Allowed origins (supports globs in your app logic)
	#[serde(default)]
	pub allowed_origins: Vec<String>,

	/// Preflight cache duration in seconds
	#[serde(default)]
	pub max_age_seconds: Option<u64>,
}
