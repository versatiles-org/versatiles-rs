use crate::{napi_result, types::ServerOptions};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles::{Config, server::TileServer as RustTileServer};
use versatiles_container::ContainerRegistry;

/// HTTP tile server for serving tiles and static content
#[napi]
pub struct TileServer {
	inner: Arc<Mutex<Option<RustTileServer>>>,
	registry: ContainerRegistry,
	port: Arc<Mutex<u16>>,
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

		// Create initial config
		let mut config = Config::default();
		config.server.ip = Some(opts.ip.unwrap_or_else(|| "0.0.0.0".to_string()));
		config.server.port = Some(opts.port.unwrap_or(8080) as u16);
		config.server.minimal_recompression = opts.minimal_recompression;

		let port = config.server.port.unwrap_or(8080);

		Ok(Self {
			inner: Arc::new(Mutex::new(None)),
			registry,
			port: Arc::new(Mutex::new(port)),
		})
	}

	/// Add a tile source to the server
	///
	/// The tiles will be served at /tiles/{name}/...
	#[napi]
	pub async fn add_tile_source(&self, name: String, path: String) -> Result<()> {
		let reader = napi_result!(self.registry.get_reader_from_str(&path).await)?;

		let mut server_lock = self.inner.lock().await;

		if let Some(ref mut server) = *server_lock {
			napi_result!(server.add_tile_source(&name, reader))?;
		} else {
			return Err(Error::from_reason(
				"Cannot add tile source before server is created. Call start() to create the server first, or use a different API.",
			));
		}

		Ok(())
	}

	/// Add a static file source to the server
	///
	/// Serves static files from a path (can be a .tar or directory)
	#[napi]
	pub async fn add_static_source(&self, path: String, url_prefix: Option<String>) -> Result<()> {
		let path_buf = PathBuf::from(&path);
		let prefix = url_prefix.unwrap_or_else(|| "/".to_string());

		let mut server_lock = self.inner.lock().await;

		if let Some(ref mut server) = *server_lock {
			napi_result!(server.add_static_source(&path_buf, &prefix))?;
		} else {
			return Err(Error::from_reason(
				"Cannot add static source before server is created. Call start() to create the server first.",
			));
		}

		Ok(())
	}

	/// Start the HTTP server
	#[napi]
	pub async fn start(&self) -> Result<()> {
		let mut server_lock = self.inner.lock().await;

		if server_lock.is_some() {
			return Err(Error::from_reason("Server is already running"));
		}

		let mut config = Config::default();
		let port_val = *self.port.lock().await;
		config.server.port = Some(port_val);

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
