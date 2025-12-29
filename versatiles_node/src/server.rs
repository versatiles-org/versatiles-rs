//! HTTP tile server
//!
//! This module provides the [`TileServer`] class for serving tiles and static
//! content over HTTP. The server supports hot-reloading of tile sources and
//! can serve multiple tile containers simultaneously.
//!
//! ## Features
//!
//! - **Multiple tile sources**: Serve different tile sets at different endpoints
//! - **Static file serving**: Serve static files from directories or TAR archives
//! - **Hot reload**: Add/remove sources without restarting the server
//! - **Compression support**: Automatic tile recompression based on client requests
//! - **TileJSON**: Automatic TileJSON metadata endpoints for each tile source
//!
//! ## URL Structure
//!
//! - Tiles: `/tiles/{name}/{z}/{x}/{y}` - Individual tiles
//! - TileJSON: `/tiles/{name}/tiles.json` - Metadata for tile source
//! - Static files: Served according to configured URL prefixes

use crate::{napi_result, runtime::create_runtime, types::ServerOptions};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles::{config::Config, server::TileServer as RustTileServer};
use versatiles_container::{DataLocation, DataSource, TilesRuntime};

// Type aliases for complex types
type TileSourceList = Arc<Mutex<Vec<(String, String)>>>;
type StaticSourceList = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// HTTP tile server for serving tiles and static content
///
/// A high-performance HTTP server for serving map tiles and static files.
/// Supports multiple tile sources with hot-reload capability, allowing you
/// to add or remove tile sources without restarting the server.
///
/// # URL Endpoints
///
/// When a tile source named "osm" is added, the following endpoints become available:
/// - `GET /tiles/osm/{z}/{x}/{y}` - Retrieve a tile
/// - `GET /tiles/osm/tiles.json` - TileJSON metadata
///
/// Static files are served according to their configured URL prefix.
///
/// # Examples
///
/// ```javascript
/// const server = new TileServer({ port: 8080 });
///
/// // Add tile sources
/// await server.addTileSource('osm', 'tiles/osm.versatiles');
/// await server.addTileSource('terrain', 'tiles/terrain.mbtiles');
///
/// // Add static content
/// await server.addStaticSource('public/', '/');
///
/// // Start the server
/// await server.start();
/// console.log(`Server running on port ${await server.port}`);
///
/// // Hot reload: add more sources while running
/// await server.addTileSource('satellite', 'tiles/satellite.pmtiles');
///
/// // Clean up
/// await server.stop();
/// ```
#[napi]
pub struct TileServer {
	inner: Arc<Mutex<Option<RustTileServer>>>,
	runtime: TilesRuntime,
	port: Arc<Mutex<u16>>,
	ip: Arc<Mutex<String>>,
	minimal_recompression: Arc<Mutex<Option<bool>>>,
	// Track accumulated sources to rebuild config on start
	tile_sources: TileSourceList,     // Vec of (name, path)
	static_sources: StaticSourceList, // Vec of (path, url_prefix)
}

#[napi]
impl TileServer {
	/// Create a new tile server
	///
	/// Creates a new HTTP tile server with the specified configuration.
	/// The server is not started until `start()` is called.
	///
	/// # Arguments
	///
	/// * `options` - Optional server configuration
	///   - `ip`: IP address to bind to (default: "0.0.0.0")
	///   - `port`: Port to listen on (default: 8080)
	///   - `minimalRecompression`: Use minimal recompression for better performance (default: false)
	///
	/// # Examples
	///
	/// ```javascript
	/// // Default settings (0.0.0.0:8080)
	/// const server = new TileServer();
	///
	/// // Custom port
	/// const server = new TileServer({ port: 3000 });
	///
	/// // Custom IP and port with minimal recompression
	/// const server = new TileServer({
	///   ip: '127.0.0.1',
	///   port: 8080,
	///   minimalRecompression: true
	/// });
	/// ```
	#[napi(constructor)]
	pub fn new(options: Option<ServerOptions>) -> Result<Self> {
		let opts = options.unwrap_or(ServerOptions {
			ip: None,
			port: None,
			minimal_recompression: None,
		});

		let runtime = create_runtime();
		let ip = opts.ip.unwrap_or_else(|| "0.0.0.0".to_string());
		let port = opts.port.unwrap_or(8080) as u16;
		let minimal_recompression = opts.minimal_recompression;

		Ok(Self {
			inner: Arc::new(Mutex::new(None)),
			runtime,
			port: Arc::new(Mutex::new(port)),
			ip: Arc::new(Mutex::new(ip)),
			minimal_recompression: Arc::new(Mutex::new(minimal_recompression)),
			tile_sources: Arc::new(Mutex::new(Vec::new())),
			static_sources: Arc::new(Mutex::new(Vec::new())),
		})
	}

	/// Add a tile source to the server
	///
	/// The tiles will be served at /tiles/{name}/...
	/// Sources can be added before or after starting the server.
	/// Changes take effect immediately without requiring a restart (hot reload).
	#[napi]
	pub async fn add_tile_source(&self, name: String, path: String) -> Result<()> {
		// Get reader to validate that the file exists
		let reader = napi_result!(self.runtime.get_reader_from_str(&path).await)?;

		// Store the source in our list (source of truth)
		let mut sources = self.tile_sources.lock().await;
		sources.push((name.clone(), path));
		drop(sources); // Release lock before potentially slow operation

		// If server is running, add the source directly for hot reload
		let mut server_lock = self.inner.lock().await;
		if let Some(server) = server_lock.as_mut() {
			napi_result!(server.add_tile_source(name, reader).await)?;
		}

		Ok(())
	}

	/// Remove a tile source from the server
	///
	/// Changes take effect immediately without requiring a restart (hot reload).
	/// Returns true if the source was found and removed, false otherwise.
	#[napi]
	pub async fn remove_tile_source(&self, name: String) -> Result<bool> {
		// Remove from our list (source of truth)
		let mut sources = self.tile_sources.lock().await;
		let initial_len = sources.len();
		sources.retain(|(n, _)| n != &name);
		let was_removed = sources.len() < initial_len;
		drop(sources);

		// If server is running, remove the source directly for hot reload
		let mut server_lock = self.inner.lock().await;
		if let Some(server) = server_lock.as_mut() {
			napi_result!(server.remove_tile_source(&name).await)?;
		}

		Ok(was_removed)
	}

	/// Add a static file source to the server
	///
	/// Serves static files from a path (can be a .tar or directory)
	/// Changes take effect immediately without requiring a restart (hot reload).
	#[napi]
	pub async fn add_static_source(&self, path: String, url_prefix: Option<String>) -> Result<()> {
		let prefix = url_prefix.clone().unwrap_or_else(|| "/".to_string());

		// Validate that the path exists
		let data_location = DataLocation::from(path.clone());
		let path_buf = napi_result!(data_location.as_path())?;
		if !path_buf.exists() {
			return Err(Error::from_reason(format!(
				"Static source path does not exist: {}",
				path
			)));
		}

		// Store the source in our list (source of truth)
		let mut sources = self.static_sources.lock().await;
		sources.push((path.clone(), url_prefix.clone()));
		drop(sources);

		// If server is running, add the source directly for hot reload
		let mut server_lock = self.inner.lock().await;
		if let Some(server) = server_lock.as_mut() {
			napi_result!(server.add_static_source(path_buf, &prefix).await)?;
		}

		Ok(())
	}

	/// Remove a static file source from the server by URL prefix
	///
	/// Changes take effect immediately without requiring a restart (hot reload).
	/// Returns true if the source was found and removed, false otherwise.
	#[napi]
	pub async fn remove_static_source(&self, url_prefix: String) -> Result<bool> {
		// Remove from our list (source of truth)
		let mut sources = self.static_sources.lock().await;
		let initial_len = sources.len();
		sources.retain(|(_, p)| p.as_ref() != Some(&url_prefix));
		let was_removed = sources.len() < initial_len;
		drop(sources);

		// If server is running, remove the source directly for hot reload
		let mut server_lock = self.inner.lock().await;
		if let Some(server) = server_lock.as_mut() {
			napi_result!(server.remove_static_source(&url_prefix).await)?;
		}

		Ok(was_removed)
	}

	/// Start the HTTP server
	///
	/// Starts the HTTP server and begins listening for requests on the configured
	/// IP address and port. All tile and static sources that have been added will
	/// be immediately available.
	///
	/// # Returns
	///
	/// Returns `Ok(())` if the server started successfully
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The server is already running
	/// - The port is already in use
	/// - Unable to bind to the specified IP address
	/// - Configuration is invalid
	///
	/// # Examples
	///
	/// ```javascript
	/// const server = new TileServer({ port: 8080 });
	/// await server.addTileSource('osm', 'tiles.versatiles');
	/// await server.start();
	/// console.log('Server started successfully');
	/// ```
	#[napi]
	pub async fn start(&self) -> Result<()> {
		let mut server_lock = self.inner.lock().await;

		if server_lock.is_some() {
			return Err(Error::from_reason("Server is already running"));
		}

		// Build config with all accumulated sources
		let mut config = Config::default();
		let port_val = *self.port.lock().await;
		let ip_val = self.ip.lock().await.clone();
		let min_recomp = *self.minimal_recompression.lock().await;

		config.server.port = Some(port_val);
		config.server.ip = Some(ip_val);
		config.server.minimal_recompression = min_recomp;

		// Add all tile sources to config
		let tile_sources = self.tile_sources.lock().await;
		for (name, path) in tile_sources.iter() {
			use versatiles::config::TileSourceConfig;
			let data_source = napi_result!(DataSource::try_from(path.as_str()))?;
			config.tile_sources.push(TileSourceConfig {
				name: Some(name.clone()),
				src: data_source,
			});
		}

		// Add all static sources to config
		let static_sources = self.static_sources.lock().await;
		for (path, url_prefix) in static_sources.iter() {
			use versatiles::config::StaticSourceConfig;
			let data_location = DataLocation::from(path.clone());
			config.static_sources.push(StaticSourceConfig {
				src: data_location,
				prefix: url_prefix.clone(),
			});
		}

		let mut server = napi_result!(RustTileServer::from_config(config, self.runtime.clone()).await)?;

		napi_result!(server.start().await)?;

		// Update the actual port if we used 0 (ephemeral)
		let actual_port = server.get_url_mapping().await;
		if !actual_port.is_empty() {
			// For now, we'll keep the port as configured
			// A better implementation would extract the actual port from the server
		}

		*server_lock = Some(server);

		Ok(())
	}

	/// Stop the HTTP server gracefully
	///
	/// Gracefully shuts down the HTTP server, completing any in-flight requests
	/// before closing. After stopping, the server can be restarted by calling
	/// `start()` again.
	///
	/// If the server is not running, this method does nothing and returns successfully.
	///
	/// # Examples
	///
	/// ```javascript
	/// await server.start();
	/// // ... server is running ...
	/// await server.stop();
	/// console.log('Server stopped');
	///
	/// // Can restart later
	/// await server.start();
	/// ```
	#[napi]
	pub async fn stop(&self) -> Result<()> {
		let mut server_lock = self.inner.lock().await;

		if let Some(mut server) = server_lock.take() {
			server.stop().await;
		}

		Ok(())
	}

	/// Get the port the server is listening on
	///
	/// Returns the port number the server is configured to use or is currently
	/// listening on. If the server was configured with port 0 (ephemeral port),
	/// this will return the actual port assigned by the operating system after
	/// the server has started.
	///
	/// # Returns
	///
	/// The port number (1-65535)
	///
	/// # Examples
	///
	/// ```javascript
	/// const server = new TileServer({ port: 8080 });
	/// console.log(`Configured port: ${await server.port}`); // 8080
	///
	/// await server.start();
	/// console.log(`Server listening on port: ${await server.port}`); // 8080
	/// ```
	#[napi(getter)]
	pub async fn port(&self) -> u32 {
		let server_lock = self.inner.lock().await;

		// If server is running, try to get the actual bound port from it
		if let Some(server) = &*server_lock {
			// The RustTileServer struct has a private port field, but we can access it
			// by trying to get the URL mapping and parsing, or we just return the configured port
			// after it's been updated by the start() method.
			// For now, return the configured port which should be updated after binding
			server.get_port() as u32
		} else {
			// If server isn't running, return the configured port
			*self.port.lock().await as u32
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_new_server_default_options() {
		let server = TileServer::new(None).unwrap();
		// Verify the server was created successfully
		assert!(server.inner.try_lock().is_ok());
	}

	#[test]
	fn test_new_server_custom_port() {
		let options = ServerOptions {
			ip: None,
			port: Some(3000),
			minimal_recompression: None,
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Verify custom port was set
		let rt = tokio::runtime::Runtime::new().unwrap();
		let port = rt.block_on(server.port());
		assert_eq!(port, 3000);
	}

	#[test]
	fn test_new_server_custom_ip() {
		let options = ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: None,
			minimal_recompression: None,
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Verify custom IP was set
		let rt = tokio::runtime::Runtime::new().unwrap();
		let ip = rt.block_on(async { server.ip.lock().await.clone() });
		assert_eq!(ip, "127.0.0.1");
	}

	#[test]
	fn test_new_server_minimal_recompression() {
		let options = ServerOptions {
			ip: None,
			port: None,
			minimal_recompression: Some(true),
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Verify minimal recompression was set
		let rt = tokio::runtime::Runtime::new().unwrap();
		let minimal_recomp = rt.block_on(async { *server.minimal_recompression.lock().await });
		assert_eq!(minimal_recomp, Some(true));
	}

	#[test]
	fn test_new_server_all_custom_options() {
		let options = ServerOptions {
			ip: Some("0.0.0.0".to_string()),
			port: Some(9999),
			minimal_recompression: Some(false),
		};
		let server = TileServer::new(Some(options)).unwrap();

		let rt = tokio::runtime::Runtime::new().unwrap();
		let port = rt.block_on(server.port());
		let ip = rt.block_on(async { server.ip.lock().await.clone() });
		let minimal_recomp = rt.block_on(async { *server.minimal_recompression.lock().await });

		assert_eq!(port, 9999);
		assert_eq!(ip, "0.0.0.0");
		assert_eq!(minimal_recomp, Some(false));
	}

	#[tokio::test]
	async fn test_port_getter_before_start() {
		let options = ServerOptions {
			ip: None,
			port: Some(8080),
			minimal_recompression: None,
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Port should return configured value even before server starts
		let port = server.port().await;
		assert_eq!(port, 8080);
	}

	#[tokio::test]
	async fn test_add_tile_source_invalid_path() {
		let server = TileServer::new(None).unwrap();

		// Try to add a non-existent tile source
		let result = server
			.add_tile_source("test".to_string(), "/nonexistent/path.mbtiles".to_string())
			.await;

		// Should fail because file doesn't exist
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_add_tile_source_valid_path() {
		let server = TileServer::new(None).unwrap();

		// Use a real test file
		let result = server
			.add_tile_source("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await;

		// Should succeed
		assert!(result.is_ok());

		// Verify it was added to the list
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert_eq!(sources[0].0, "berlin");
	}

	#[tokio::test]
	async fn test_add_multiple_tile_sources() {
		let server = TileServer::new(None).unwrap();

		// Add first source
		server
			.add_tile_source("berlin1".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Add second source
		server
			.add_tile_source("berlin2".to_string(), "../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		// Verify both were added
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 2);
		assert_eq!(sources[0].0, "berlin1");
		assert_eq!(sources[1].0, "berlin2");
	}

	#[tokio::test]
	async fn test_remove_tile_source_not_found() {
		let server = TileServer::new(None).unwrap();

		// Try to remove a source that doesn't exist
		let result = server.remove_tile_source("nonexistent".to_string()).await;

		// Should return false (not found)
		assert!(!result.unwrap());
	}

	#[tokio::test]
	async fn test_remove_tile_source_success() {
		let server = TileServer::new(None).unwrap();

		// Add a source
		server
			.add_tile_source("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Verify it was added
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
		drop(sources);

		// Remove the source
		let result = server.remove_tile_source("berlin".to_string()).await;

		// Should return true (found and removed)
		assert!(result.unwrap());

		// Verify it was removed
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 0);
	}

	#[tokio::test]
	async fn test_remove_tile_source_specific_from_multiple() {
		let server = TileServer::new(None).unwrap();

		// Add multiple sources
		server
			.add_tile_source("berlin1".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();
		server
			.add_tile_source("berlin2".to_string(), "../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		// Remove only the first one
		server.remove_tile_source("berlin1".to_string()).await.unwrap();

		// Verify only berlin2 remains
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert_eq!(sources[0].0, "berlin2");
	}

	#[tokio::test]
	async fn test_add_static_source_invalid_path() {
		let server = TileServer::new(None).unwrap();

		// Try to add a non-existent static source
		let result = server
			.add_static_source("/nonexistent/path".to_string(), Some("/".to_string()))
			.await;

		// Should fail because path doesn't exist
		assert!(result.is_err());
		let err = result.unwrap_err();
		assert!(err.to_string().contains("does not exist"));
	}

	#[tokio::test]
	async fn test_add_static_source_valid_path() {
		let server = TileServer::new(None).unwrap();

		// Use a real test file
		let result = server
			.add_static_source("../testdata/static.tar.gz".to_string(), Some("/static".to_string()))
			.await;

		// Should succeed
		assert!(result.is_ok());

		// Verify it was added to the list
		let sources = server.static_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert_eq!(sources[0].0, "../testdata/static.tar.gz");
		assert_eq!(sources[0].1, Some("/static".to_string()));
	}

	#[tokio::test]
	async fn test_add_static_source_default_prefix() {
		let server = TileServer::new(None).unwrap();

		// Add static source without specifying prefix
		server
			.add_static_source("../testdata/static.tar.gz".to_string(), None)
			.await
			.unwrap();

		// Verify default prefix was used
		let sources = server.static_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert_eq!(sources[0].1, None);
	}

	#[tokio::test]
	async fn test_remove_static_source_not_found() {
		let server = TileServer::new(None).unwrap();

		// Try to remove a source that doesn't exist
		let result = server.remove_static_source("/nonexistent".to_string()).await;

		// Should return false (not found)
		assert!(!result.unwrap());
	}

	#[tokio::test]
	async fn test_remove_static_source_success() {
		let server = TileServer::new(None).unwrap();

		// Add a static source
		server
			.add_static_source("../testdata/static.tar.gz".to_string(), Some("/static".to_string()))
			.await
			.unwrap();

		// Remove it
		let result = server.remove_static_source("/static".to_string()).await;

		// Should return true (found and removed)
		assert!(result.unwrap());

		// Verify it was removed
		let sources = server.static_sources.lock().await;
		assert_eq!(sources.len(), 0);
	}

	#[tokio::test]
	async fn test_stop_when_not_running() {
		let server = TileServer::new(None).unwrap();

		// Stop a server that was never started
		let result = server.stop().await;

		// Should succeed (no-op)
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_start_already_running() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0), // Use ephemeral port to avoid conflicts
			minimal_recompression: None,
		}))
		.unwrap();

		// Start the server
		server.start().await.unwrap();

		// Try to start again while running
		let result = server.start().await;

		// Should fail with "already running" error
		assert!(result.is_err());
		let err = result.unwrap_err();
		assert!(err.to_string().contains("already running"));

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_server_start_stop_lifecycle() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0), // Use ephemeral port
			minimal_recompression: None,
		}))
		.unwrap();

		// Initially not running
		{
			let server_lock = server.inner.lock().await;
			assert!(server_lock.is_none());
		}

		// Start the server
		server.start().await.unwrap();

		// Should be running now
		{
			let server_lock = server.inner.lock().await;
			assert!(server_lock.is_some());
		}

		// Stop the server
		server.stop().await.unwrap();

		// Should not be running
		{
			let server_lock = server.inner.lock().await;
			assert!(server_lock.is_none());
		}
	}

	#[tokio::test]
	async fn test_server_restart() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Start, stop, start again
		server.start().await.unwrap();
		server.stop().await.unwrap();
		let result = server.start().await;

		// Should be able to restart
		assert!(result.is_ok());

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_add_tile_source_after_start() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Start server first
		server.start().await.unwrap();

		// Add source after starting (hot reload)
		let result = server
			.add_tile_source("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await;

		// Should succeed
		assert!(result.is_ok());

		// Verify it was added
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_remove_tile_source_after_start() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Add source before starting
		server
			.add_tile_source("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Start server
		server.start().await.unwrap();

		// Remove source after starting (hot reload)
		let result = server.remove_tile_source("berlin".to_string()).await;

		// Should succeed
		assert!(result.unwrap());

		// Verify it was removed
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 0);

		// Clean up
		server.stop().await.unwrap();
	}
}
