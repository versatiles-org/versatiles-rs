//! HTTP server configuration for the VersaTiles server.
//!
//! This module defines the [`ServerConfig`] struct, which configures the basic
//! networking and API behavior of the HTTP server. It can be parsed from YAML
//! or JSON and included in the top-level [`Config`](crate::config::Config).
//!
//! # Example YAML
//! ```yaml
//! server:
//!   ip: 0.0.0.0
//!   port: 8080
//!   minimal_recompression: false
//!   disable_api: false
//! ```
//!
//! All fields are optional. Defaults are applied when values are not specified.

use std::str;

use serde::Deserialize;
use versatiles_derive::ConfigDoc;

/// Configuration for the VersaTiles HTTP server.
///
/// This configuration controls which address and port the server listens on,
/// whether recompression prioritizes speed or ratio, and whether the `/api`
/// endpoints are disabled.
///
/// # Fields
/// * `ip` — Optional IP address to bind to (default `"0.0.0.0"`).
/// * `port` — Optional port to listen on (default `8080`).
/// * `minimal_recompression` — If `true`, prefer faster compression over smaller output.
/// * `disable_api` — If `true`, disable the `/api` endpoints entirely.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
	/// Optional IP address to bind to
	/// Defaults to "0.0.0.0"
	#[serde()]
	#[config_demo("0.0.0.0")]
	pub ip: Option<String>,

	/// Optional HTTP server port
	/// Defaults to 8080
	#[serde()]
	#[config_demo("8080")]
	pub port: Option<u16>,

	/// Optional flag to prefer faster compression over smaller size
	/// Defaults to false (smaller compression)
	#[serde()]
	#[config_demo("false")]
	pub minimal_recompression: Option<bool>,

	/// Optional flag to disable the `/api` endpoints
	/// Defaults to false (enabling the API)
	#[serde()]
	#[config_demo("false")]
	pub disable_api: Option<bool>,

	/// Optional `Cache-Control` header sent with every tile
	/// Defaults to "public, max-age=2419200, no-transform" (four weeks)
	///
	/// Tile URLs are stable — built from a mount name and a coordinate — so a
	/// client answers from its own cache when the tiles behind a mount change.
	/// Four weeks suits a public, CDN-fronted server; a preview or a test
	/// harness serving tiles that change wants something shorter, e.g.
	/// "no-cache".
	#[serde()]
	#[config_demo("public, max-age=2419200, no-transform")]
	pub cache_control: Option<String>,
}

/// Helper methods for merging partial `ServerConfig` values.
///
/// These methods selectively override fields only if the provided values are `Some`.
/// Used when combining multiple configuration sources (e.g., defaults, CLI, and YAML).
impl ServerConfig {
	/// Overrides the address to bind to, if a value is given.
	pub fn override_optional_ip(&mut self, ip: &Option<String>) {
		if ip.is_some() {
			self.ip.clone_from(ip);
		}
	}
	/// Overrides the `Cache-Control` header sent with tiles, if a value is given.
	pub fn override_optional_cache_control(&mut self, cache_control: &Option<String>) {
		if cache_control.is_some() {
			self.cache_control.clone_from(cache_control);
		}
	}
	/// Overrides the port to listen on, if a value is given.
	pub fn override_optional_port(&mut self, port: &Option<u16>) {
		if port.is_some() {
			self.port = *port;
		}
	}
	/// Overrides whether to recompress as little as possible, if a value is given.
	pub fn override_optional_minimal_recompression(&mut self, minimal_recompression: &Option<bool>) {
		if minimal_recompression.is_some() {
			self.minimal_recompression = *minimal_recompression;
		}
	}
	/// Overrides whether the JSON API is switched off, if a value is given.
	pub fn override_optional_disable_api(&mut self, disable_api: &Option<bool>) {
		if disable_api.is_some() {
			self.disable_api = *disable_api;
		}
	}
}
