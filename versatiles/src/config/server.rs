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

use serde::Deserialize;
use std::str;
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
}

/// Helper methods for merging partial `ServerConfig` values.
///
/// These methods selectively override fields only if the provided values are `Some`.
/// Used when combining multiple configuration sources (e.g., defaults, CLI, and YAML).
impl ServerConfig {
	pub fn override_optional_ip(&mut self, ip: &Option<String>) {
		if ip.is_some() {
			self.ip.clone_from(ip);
		}
	}
	pub fn override_optional_port(&mut self, port: &Option<u16>) {
		if port.is_some() {
			self.port = *port;
		}
	}
	pub fn override_optional_minimal_recompression(&mut self, minimal_recompression: &Option<bool>) {
		if minimal_recompression.is_some() {
			self.minimal_recompression = *minimal_recompression;
		}
	}
	pub fn override_optional_disable_api(&mut self, disable_api: &Option<bool>) {
		if disable_api.is_some() {
			self.disable_api = *disable_api;
		}
	}
}
