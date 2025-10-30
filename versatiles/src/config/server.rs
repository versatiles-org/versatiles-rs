use serde::Deserialize;
use std::str;

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
	/// IP to bind to.
	#[serde()]
	pub ip: Option<String>,

	/// TCP port to bind to.
	#[serde()]
	pub port: Option<u16>,

	/// Whether to prefer faster (vs. smaller) compression.
	#[serde()]
	pub minimal_recompression: Option<bool>,

	/// Disable API.
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
