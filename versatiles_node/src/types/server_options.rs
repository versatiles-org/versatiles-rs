use napi_derive::napi;

/// Configuration options for the tile server
///
/// All fields are optional and will use sensible defaults if not specified.
#[napi(object)]
pub struct ServerOptions {
	/// IP address or hostname to bind to
	///
	/// Determines which network interface the server listens on:
	/// - `"0.0.0.0"`: Listen on all network interfaces (accessible from any network)
	/// - `"127.0.0.1"`: Listen only on localhost (local access only)
	/// - Specific IP: Listen only on that interface
	///
	/// **Security Note:** Using `"0.0.0.0"` makes the server accessible from the network.
	/// Use `"127.0.0.1"` for development or when using a reverse proxy.
	///
	/// **Default:** `"0.0.0.0"` (all interfaces)
	pub ip: Option<String>,

	/// TCP port number to listen on (1-65535)
	///
	/// The port where the HTTP server will accept connections.
	/// - Use `0` to let the OS assign an available port automatically (ephemeral port)
	/// - Ports below 1024 typically require administrator/root privileges
	/// - Common choices: 8080, 3000, 8000
	///
	/// **Default:** `8080`
	pub port: Option<u32>,

	/// Enable minimal recompression for improved performance
	///
	/// When enabled, the server performs minimal tile recompression to match
	/// client requirements, favoring speed over optimal compression ratio:
	/// - Tiles are served with their original compression when possible
	/// - Only recompresses when absolutely necessary for client compatibility
	/// - Reduces CPU usage and improves response times
	/// - May result in slightly larger tile sizes sent to clients
	///
	/// When disabled (default), the server optimally recompresses tiles to match
	/// the client's preferred compression format, which provides better bandwidth
	/// efficiency but uses more CPU.
	///
	/// **Recommended:** Enable for high-traffic servers or when CPU is limited.
	///
	/// **Default:** `false` (optimal recompression)
	pub minimal_recompression: Option<bool>,
}
