//! Cross-Origin Resource Sharing (CORS) configuration for the VersaTiles server.
//!
//! This configuration determines which origins are allowed to make cross-origin requests
//! to the API and static endpoints. It maps directly to HTTP `Access-Control-*` headers.
//!
//! By default, all origins are allowed (`*`). You can restrict access by specifying
//! a list of allowed origins.
//!
//! Typical usage:
//! - Allowing all origins (default): `["*"]`
//! - Restricting to trusted domains (e.g., `https://example.org`)
//! - Allowing wildcard subdomains (e.g., `https://*.example.org`)
//! - Enabling short-lived preflight cache times during development
//!
//! The `Cors` struct can be parsed from YAML or JSON using Serde.
//!
//! # Example YAML
//! ```yaml
//! cors:
//!   allowed_origins:
//!     - "https://example.org"
//!     - "https://*.example.net"
//!   max_age_seconds: 86400
//! ```
use serde::Deserialize;
use versatiles_derive::ConfigDoc;

/// CORS policy configuration.
///
/// The server uses this configuration to build the `CorsLayer` that controls which
/// origins can access resources via browser-based requests.
///
/// - `allowed_origins`: A list of permitted origins, globs, or regular expressions.
///   Defaults to `["*"]` (all origins allowed).
/// - `max_age_seconds`: Duration that browsers should cache preflight responses.
#[derive(Debug, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct CorsConfig {
	/// Allowed origins for CORS requests
	/// Defaults to `["*"]` (all origins allowed).
	/// Supports:
	/// - `*` to allow all origins
	/// - Exact origins like `https://example.com`
	/// - Subdomains like `https://*.example.com` (matched on host labels)
	/// - Any port on one host like `https://example.com:*`
	/// - `null` for the null origin (sandboxed iframes, `file://`)
	/// - Regular expressions enclosed in slashes like `/^https:\/\/example\.com$/`
	///
	/// Two older forms still work but match raw string edges rather than the
	/// parts of an origin, and are open at one end: `https://example.com*` also
	/// allows `https://example.com.attacker.test`, and `*example.com` also
	/// allows `https://notexample.com`. A scheme-less `*.example.com` is open in
	/// a third way — it allows `http://` as readily as `https://`, while never
	/// matching an origin that carries a port. All three log a warning at
	/// startup. `https://*.example.com` and `https://example.com:*` say what
	/// those are usually reached for, without the open end. An unanchored regular
	/// expression matches any origin *containing* it, so wrap it in `^...$`.
	#[serde(default = "default_allowed_origins")]
	#[config_demo(
		r#"
    - "https://example.org"
    - "https://*.example.net""#
	)]
	pub allowed_origins: Vec<String>,

	/// Optional duration for preflight cache in seconds
	/// Defaults to 86400 (1 day)
	#[serde(default)]
	#[config_demo("86400")]
	pub max_age_seconds: Option<u64>,
}

fn default_allowed_origins() -> Vec<String> {
	vec!["*".to_string()]
}

impl Default for CorsConfig {
	fn default() -> Self {
		Self {
			allowed_origins: default_allowed_origins(),
			max_age_seconds: None,
		}
	}
}
