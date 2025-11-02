use serde::Deserialize;
use std::str;
use versatiles_derive::ConfigDoc;

#[derive(Debug, Default, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
	/// Defines the IP address to bind to. Use "0.0.0.0" to listen on all interfaces.
	#[serde()]
	pub ip: Option<String>,

	/// Sets the HTTP server port. Defaults to 8080 if not specified.
	#[serde()]
	pub port: Option<u16>,

	/// Controls whether to prefer faster (vs. smaller) compression. Defaults to false (smaller compression).
	#[serde()]
	pub minimal_recompression: Option<bool>,

	/// Disables the `/api` endpoints, leaving only static and tile routes enabled.
	#[serde()]
	pub disable_api: Option<bool>,
}

impl ServerConfig {
	pub fn override_optional_ip(&mut self, ip: &Option<String>) {
		if ip.is_some() {
			self.ip = ip.clone();
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
