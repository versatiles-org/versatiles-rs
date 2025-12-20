use napi_derive::napi;

/// Options for tile server
#[napi(object)]
pub struct ServerOptions {
	/// IP address to bind (default: "0.0.0.0")
	pub ip: Option<String>,
	/// Port to listen on (default: 8080)
	pub port: Option<u32>,
	/// Use minimal recompression for better performance
	pub minimal_recompression: Option<bool>,
}
