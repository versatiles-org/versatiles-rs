use crate::{napi_result, types::ServerOptions};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles::{Config, server::TileServer as RustTileServer};
use versatiles_container::{ContainerRegistry, DataSource, DataLocation};

/// HTTP tile server for serving tiles and static content
#[napi]
pub struct TileServer {
	inner: Arc<Mutex<Option<RustTileServer>>>,
	registry: ContainerRegistry,
	port: Arc<Mutex<u16>>,
	ip: Arc<Mutex<String>>,
	minimal_recompression: Arc<Mutex<Option<bool>>>,
	// Track accumulated sources to rebuild config on start
	tile_sources: Arc<Mutex<Vec<(String, String)>>>,  // Vec of (name, path)
	static_sources: Arc<Mutex<Vec<(String, Option<String>)>>>,  // Vec of (path, url_prefix)
}

#[napi]
impl TileServer {
	/// Create a new tile server
	#[napi(constructor)]
	pub fn new(options: Option<ServerOptions>) -> Result<Self> {
		let opts = options.unwrap_or(ServerOptions {
			ip: None,
			port: None,
			minimal_recompression: None,
		});

		let registry = ContainerRegistry::default();
		let ip = opts.ip.unwrap_or_else(|| "0.0.0.0".to_string());
		let port = opts.port.unwrap_or(8080) as u16;
		let minimal_recompression = opts.minimal_recompression;

		Ok(Self {
			inner: Arc::new(Mutex::new(None)),
			registry,
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
	/// If added after start(), call stop() and start() again to reload.
	#[napi]
	pub async fn add_tile_source(&self, name: String, path: String) -> Result<()> {
		// Validate that the file exists by trying to open it
		let _ = napi_result!(self.registry.get_reader_from_str(&path).await)?;

		// Store the source in our list
		let mut sources = self.tile_sources.lock().await;
		sources.push((name, path));

		Ok(())
	}

	/// Add a static file source to the server
	///
	/// Serves static files from a path (can be a .tar or directory)
	/// Sources can be added before or after starting the server.
	/// If added after start(), call stop() and start() again to reload.
	#[napi]
	pub async fn add_static_source(&self, path: String, url_prefix: Option<String>) -> Result<()> {
		// Validate that the path exists
		let data_location = DataLocation::from(path.clone());
		if let Ok(path_ref) = data_location.as_path() {
			if !path_ref.exists() {
				return Err(Error::from_reason(format!(
					"Static source path does not exist: {}",
					path
				)));
			}
		}

		// Store the source in our list
		let mut sources = self.static_sources.lock().await;
		sources.push((path, url_prefix));

		Ok(())
	}

	/// Start the HTTP server
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
			use versatiles::TileSourceConfig;
			let data_source = napi_result!(DataSource::try_from(path.as_str()))?;
			config.tile_sources.push(TileSourceConfig {
				name: Some(name.clone()),
				path: data_source,
			});
		}

		// Add all static sources to config
		let static_sources = self.static_sources.lock().await;
		for (path, url_prefix) in static_sources.iter() {
			use versatiles::StaticSourceConfig;
			let data_location = DataLocation::from(path.clone());
			config.static_sources.push(StaticSourceConfig {
				path: data_location,
				url_prefix: url_prefix.clone(),
			});
		}

		let mut server = napi_result!(RustTileServer::from_config(config, self.registry.clone()).await)?;

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
	#[napi]
	pub async fn stop(&self) -> Result<()> {
		let mut server_lock = self.inner.lock().await;

		if let Some(mut server) = server_lock.take() {
			server.stop().await;
		}

		Ok(())
	}

	/// Get the port the server is listening on
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
