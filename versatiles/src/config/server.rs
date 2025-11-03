use serde::Deserialize;
use std::str;
use versatiles_derive::ConfigDoc;

#[derive(Debug, Default, Clone, Deserialize, PartialEq, ConfigDoc)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
	/// Optional IP address to bind to, defaults to "0.0.0.0".
	#[serde()]
	#[config_demo("0.0.0.0")]
	pub ip: Option<String>,

	/// Optional HTTP server port, defaults to 8080.
	#[serde()]
	#[config_demo("8080")]
	pub port: Option<u16>,

	/// Optional flag that controls whether to prefer faster (vs. smaller) compression.
	/// Defaults to false (smaller compression).
	#[serde()]
	#[config_demo("false")]
	pub minimal_recompression: Option<bool>,

	/// Optional flag that disables the `/api` endpoints.
	#[serde()]
	#[config_demo("false")]
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
