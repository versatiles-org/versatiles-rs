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
#[napi]
pub struct TileServer {
	inner: Arc<Mutex<Option<RustTileServer>>>,
	runtime: Arc<TilesRuntime>,
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
		let reader = napi_result!(self.runtime.registry().get_reader_from_str(&path).await)?;

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
