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

use crate::{napi_result, runtime::create_runtime, tile_source::TileSource, types::ServerOptions};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
use versatiles::{config::Config, server::TileServer as RustTileServer};
use versatiles_container::{DataLocation, TileSource as RustTileSource, TilesRuntime};

// Type aliases for complex types
type TileSourceList = Mutex<HashMap<String, Arc<Box<dyn RustTileSource>>>>; // Map of name -> TileSource
type StaticSourceList = Mutex<Vec<(String, Option<String>)>>; // Vec of (path, url_prefix)

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
/// console.log(`Server running on port ${server.port}`);
///
/// // Hot reload: add more sources while running
/// await server.addTileSource('satellite', 'tiles/satellite.pmtiles');
///
/// // Clean up
/// await server.stop();
/// ```
#[napi]
pub struct TileServer {
	inner: Mutex<Option<RustTileServer>>,
	runtime: TilesRuntime,
	initial_port: u16,
	actual_port: Arc<RwLock<Option<u16>>>, // Cached actual bound port for sync access
	ip: String,
	minimal_recompression: Option<bool>,
	// Track accumulated sources to rebuild config on start
	tile_sources: TileSourceList,     // Map of name -> TileSource
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
		let initial_port = opts.port.unwrap_or(8080) as u16;
		let minimal_recompression = opts.minimal_recompression;

		Ok(Self {
			inner: Mutex::new(None),
			runtime,
			initial_port,
			actual_port: Arc::new(RwLock::new(None)),
			ip,
			minimal_recompression,
			tile_sources: Mutex::new(HashMap::new()),
			static_sources: Mutex::new(Vec::new()),
		})
	}

	/// Add a tile source to the server from a TileSource object
	///
	/// The tiles will be served at /tiles/{name}/...
	///
	/// This method supports all types of TileSources:
	/// - File-based sources (MBTiles, PMTiles, VersaTiles, TAR, directories)
	/// - VPL pipeline sources (e.g., filtered or transformed tiles)
	///
	/// All sources support hot reload and can be added before or after starting the server.
	/// Changes take effect immediately without requiring a server restart.
	///
	/// # Errors
	///
	/// Returns an error if a tile source with the given name already exists.
	#[napi]
	pub async fn add_tile_source(&self, name: String, source: &TileSource) -> Result<()> {
		let reader = source.reader();

		// Check for duplicate name and add to our map
		let mut sources = self.tile_sources.lock().await;
		if sources.contains_key(&name) {
			return Err(Error::from_reason(format!(
				"Tile source with name '{name}' already exists",
			)));
		}

		// If server is running, add to it for hot reload
		if let Some(server) = self.inner.lock().await.as_mut() {
			napi_result!(server.add_tile_source(name.clone(), Arc::clone(&reader)).await)?;
		}

		sources.insert(name, reader);

		Ok(())
	}

	/// Add a tile source to the server from a file path
	///
	/// The tiles will be served at /tiles/{name}/...
	/// Sources can be added before or after starting the server.
	/// Changes take effect immediately without requiring a restart (hot reload).
	#[napi]
	pub async fn add_tile_source_from_path(&self, name: String, path: String) -> Result<()> {
		// Open the tile source
		let tile_source = TileSource::open(path).await?;

		// Delegate to add_tile_source
		self.add_tile_source(name, &tile_source).await
	}

	/// Remove a tile source from the server
	///
	/// Changes take effect immediately without requiring a restart (hot reload).
	/// Returns true if the source was found and removed, false otherwise.
	#[napi]
	pub async fn remove_tile_source(&self, name: String) -> Result<bool> {
		// Remove from our map (source of truth)
		let mut sources = self.tile_sources.lock().await;
		let was_removed = sources.remove(&name).is_some();
		drop(sources);

		// If server is running, remove the source directly for hot reload
		if was_removed {
			let mut server_lock = self.inner.lock().await;
			if let Some(server) = server_lock.as_mut() {
				napi_result!(server.remove_tile_source(&name))?;
			}
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
			napi_result!(server.remove_static_source(&url_prefix))?;
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

		config.server.port = Some(self.initial_port);
		config.server.ip = Some(self.ip.clone());
		config.server.minimal_recompression = self.minimal_recompression;

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

		let tile_sources = self.tile_sources.lock().await;
		for (name, tile_source) in tile_sources.iter() {
			// Add to server
			napi_result!(server.add_tile_source(name.clone(), tile_source.clone()).await)?;
		}

		// Cache the actual bound port for synchronous access
		let bound_port = server.get_port();
		*self
			.actual_port
			.write()
			.expect("Port cache RwLock poisoned - server state corrupted") = Some(bound_port);

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

		// Clear cached port when server stops
		*self
			.actual_port
			.write()
			.expect("Port cache RwLock poisoned - server state corrupted") = None;

		Ok(())
	}

	/// Get the server port
	///
	/// Returns the actual bound port if the server is running, or the configured
	/// port if the server hasn't been started yet.
	///
	/// For ephemeral ports (configured as 0), this will return the OS-assigned
	/// port after `start()` is called.
	///
	/// # Returns
	///
	/// The port number as a 32-bit unsigned integer.
	///
	/// # Examples
	///
	/// ```javascript
	/// const server = new TileServer({ port: 0 }); // Ephemeral port
	/// console.log(server.port); // 0 (before starting)
	/// await server.start();
	/// console.log(server.port); // 54321 (actual assigned port, no await needed!)
	/// ```
	#[napi(getter)]
	pub fn port(&self) -> u32 {
		// Try to read cached actual port first (when server is running)
		match self.actual_port.read() {
			Ok(guard) => {
				if let Some(actual) = *guard {
					actual as u32
				} else {
					// Fallback to configured port (before server starts)
					self.initial_port as u32
				}
			}
			Err(_) => {
				// Fallback to configured port if lock is poisoned
				self.initial_port as u32
			}
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

		// Verify custom port was set (now synchronous)
		assert_eq!(server.port(), 3000);
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
		assert_eq!(server.ip, "127.0.0.1");
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
		assert_eq!(server.minimal_recompression, Some(true));
	}

	#[test]
	fn test_new_server_all_custom_options() {
		let options = ServerOptions {
			ip: Some("0.0.0.0".to_string()),
			port: Some(9999),
			minimal_recompression: Some(false),
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Port is now synchronous
		assert_eq!(server.port(), 9999);
		assert_eq!(server.ip, "0.0.0.0");
		assert_eq!(server.minimal_recompression, Some(false));
	}

	#[test]
	fn test_port_getter_before_start() {
		let options = ServerOptions {
			ip: None,
			port: Some(8080),
			minimal_recompression: None,
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Port should return configured value even before server starts (synchronous)
		assert_eq!(server.port(), 8080);
	}

	#[test]
	fn test_port_getter_ephemeral_before_start() {
		let options = ServerOptions {
			ip: None,
			port: Some(0), // Ephemeral port
			minimal_recompression: None,
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Before start, ephemeral port should return 0
		assert_eq!(server.port(), 0);
	}

	#[tokio::test]
	async fn test_port_getter_ephemeral_after_start() {
		let options = ServerOptions {
			ip: None,
			port: Some(0), // Ephemeral port
			minimal_recompression: None,
		};
		let server = TileServer::new(Some(options)).unwrap();

		// Before start
		assert_eq!(server.port(), 0);

		// Start server
		server.start().await.unwrap();

		// After start, should return actual assigned port
		let actual_port = server.port();
		assert!(actual_port > 0, "Ephemeral port should be assigned");

		// Stop server
		server.stop().await.unwrap();

		// After stop, should return configured port (0)
		assert_eq!(server.port(), 0);
	}

	#[tokio::test]
	async fn test_add_tile_source_from_path_invalid() {
		let server = TileServer::new(None).unwrap();

		// Try to add a non-existent tile source
		let result = server
			.add_tile_source_from_path("test".to_string(), "/nonexistent/path.mbtiles".to_string())
			.await;

		// Should fail because file doesn't exist
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_add_tile_source_from_path_valid() {
		let server = TileServer::new(None).unwrap();

		// Use a real test file
		let result = server
			.add_tile_source_from_path("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await;

		// Should succeed
		assert!(result.is_ok());

		// Verify it was added to the map
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert!(sources.contains_key("berlin"));
	}

	#[tokio::test]
	async fn test_add_multiple_tile_sources_from_path() {
		let server = TileServer::new(None).unwrap();

		// Add first source
		server
			.add_tile_source_from_path("berlin1".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Add second source
		server
			.add_tile_source_from_path("berlin2".to_string(), "../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		// Verify both were added
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 2);
		assert!(sources.contains_key("berlin1"));
		assert!(sources.contains_key("berlin2"));
	}

	#[tokio::test]
	async fn test_add_duplicate_tile_source_name() {
		let server = TileServer::new(None).unwrap();

		// Add first source
		server
			.add_tile_source_from_path("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Try to add second source with same name
		let result = server
			.add_tile_source_from_path("berlin".to_string(), "../testdata/berlin.pmtiles".to_string())
			.await;

		// Should fail with duplicate name error
		assert!(result.is_err());
		let error_msg = result.unwrap_err().to_string();
		assert!(
			error_msg.contains("already exists"),
			"Error message should mention duplicate name, got: {}",
			error_msg
		);

		// Verify only one source exists
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
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
			.add_tile_source_from_path("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
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
			.add_tile_source_from_path("berlin1".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();
		server
			.add_tile_source_from_path("berlin2".to_string(), "../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		// Remove only the first one
		server.remove_tile_source("berlin1".to_string()).await.unwrap();

		// Verify only berlin2 remains
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert!(sources.contains_key("berlin2"));
		assert!(!sources.contains_key("berlin1"));
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
	async fn test_add_tile_source_from_path_after_start() {
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
			.add_tile_source_from_path("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
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
			.add_tile_source_from_path("berlin".to_string(), "../testdata/berlin.mbtiles".to_string())
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

	#[tokio::test]
	async fn test_add_tile_source_with_tile_source_object() {
		let server = TileServer::new(None).unwrap();

		// Open a TileSource
		let tile_source = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Add the TileSource object to the server
		let result = server.add_tile_source("berlin".to_string(), &tile_source).await;

		// Should succeed
		assert!(result.is_ok());

		// Verify it was added to the map
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);
		assert!(sources.contains_key("berlin"));

		// Verify the TileSource is stored
		let stored_source = sources.get("berlin").unwrap();
		let metadata = stored_source.metadata();
		assert_eq!(metadata.tile_format.as_str(), "mvt");
	}

	#[tokio::test]
	async fn test_add_tile_source_object_after_start() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Start server first
		server.start().await.unwrap();

		// Open a TileSource
		let tile_source = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Add source after starting (hot reload)
		let result = server.add_tile_source("berlin".to_string(), &tile_source).await;

		// Should succeed
		assert!(result.is_ok());

		// Verify it was added
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 1);

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_mix_tile_source_types() {
		let server = TileServer::new(None).unwrap();

		// Add from path
		server
			.add_tile_source_from_path("berlin1".to_string(), "../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Add from TileSource object
		let tile_source = TileSource::open("../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();
		server
			.add_tile_source("berlin2".to_string(), &tile_source)
			.await
			.unwrap();

		// Verify both were added
		let sources = server.tile_sources.lock().await;
		assert_eq!(sources.len(), 2);
		assert!(sources.contains_key("berlin1"));
		assert!(sources.contains_key("berlin2"));

		// Verify both are TileSource objects
		assert_eq!(sources.get("berlin1").unwrap().metadata().tile_format.as_str(), "mvt");
		assert_eq!(sources.get("berlin2").unwrap().metadata().tile_format.as_str(), "mvt");
	}

	#[tokio::test]
	async fn test_server_start_with_tile_source_objects() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Add TileSource object before starting
		let tile_source = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();
		server
			.add_tile_source("berlin".to_string(), &tile_source)
			.await
			.unwrap();

		// Start should succeed even with Source-based tile sources
		let result = server.start().await;
		assert!(result.is_ok());

		// Verify server is running
		{
			let server_lock = server.inner.lock().await;
			assert!(server_lock.is_some());
		}

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_server_with_vpl_tile_source() {
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Create a VPL-based TileSource and add it immediately (don't hold reference)
		{
			let vpl = r#"from_container filename="berlin.mbtiles" | filter level_min=5 level_max=10"#;
			let tile_source = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
				.await
				.unwrap();

			// Add VPL source to server (tile_source will be dropped after this block)
			server
				.add_tile_source("berlin_filtered".to_string(), &tile_source)
				.await
				.unwrap();
		} // tile_source is dropped here

		// Start should succeed with VPL sources
		let result = server.start().await;
		if let Err(e) = &result {
			eprintln!("Start failed: {:?}", e);
		}
		assert!(result.is_ok());

		// Verify server is running
		{
			let server_lock = server.inner.lock().await;
			assert!(server_lock.is_some());
		}

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_vpl_hot_reload() {
		// Test that VPL sources can be added after server starts (hot reload)
		let server = TileServer::new(Some(ServerOptions {
			ip: Some("127.0.0.1".to_string()),
			port: Some(0),
			minimal_recompression: None,
		}))
		.unwrap();

		// Start server first
		server.start().await.unwrap();

		// Now add a VPL source after server has started (hot reload)
		let vpl = r#"from_container filename="berlin.mbtiles" | filter level_min=5 level_max=10"#;
		let tile_source = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		// This should succeed - VPL sources now support hot reload
		let result = server
			.add_tile_source("berlin_filtered".to_string(), &tile_source)
			.await;

		assert!(result.is_ok(), "VPL hot-reload should succeed");

		// Clean up
		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_concurrent_port_reads() {
		// Test that multiple concurrent port reads work correctly
		let server = Arc::new(
			TileServer::new(Some(ServerOptions {
				port: Some(0),
				ip: None,
				minimal_recompression: None,
			}))
			.unwrap(),
		);

		server.start().await.unwrap();

		// Spawn 100 concurrent readers
		let mut handles = Vec::new();
		for _ in 0..100 {
			let server_clone = Arc::clone(&server);
			handles.push(tokio::spawn(async move {
				let port = server_clone.port();
				assert!(port > 0);
				port
			}));
		}

		// All reads should return the same port
		let mut ports = Vec::new();
		for handle in handles {
			ports.push(handle.await.unwrap());
		}

		assert!(
			ports.iter().all(|&p| p == ports[0]),
			"All concurrent reads should return the same port"
		);

		server.stop().await.unwrap();
	}

	#[tokio::test]
	async fn test_port_read_during_start() {
		// Test that reading port during server start doesn't panic
		let server = Arc::new(
			TileServer::new(Some(ServerOptions {
				port: Some(0),
				ip: None,
				minimal_recompression: None,
			}))
			.unwrap(),
		);

		// Read port before start
		assert_eq!(server.port(), 0);

		// Start server and immediately read (potential race)
		let server_clone = Arc::clone(&server);
		let read_handle = tokio::spawn(async move {
			tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
			server_clone.port()
		});

		server.start().await.unwrap();
		let port = read_handle.await.unwrap();

		// Should get either 0 (read before cache update) or actual port
		assert!(port > 0);

		server.stop().await.unwrap();
	}
}
