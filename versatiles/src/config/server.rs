use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Server {
	/// IP to bind to.
	#[serde()]
	pub ip: Option<IpAddr>,

	/// TCP port to bind to.
	#[serde()]
	pub port: Option<u16>,

	/// Whether to prefer faster (vs. smaller) compression.
	#[serde()]
	pub minimal_recompression: Option<bool>,
}
