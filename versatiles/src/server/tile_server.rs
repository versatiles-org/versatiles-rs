//! VersaTiles HTTP server lifecycle and composition.
//!
//! This module wires together the public HTTP surface for serving tiles and static assets.
//! The *logic* is intentionally split into focused modules:
//! - `handlers` implement the concrete HTTP handlers and response helpers.
//! - `routes` composes handlers into an Axum `Router`.
//! - `encoding` parses `Accept-Encoding` into our internal compression bitset.
//! - `cors` builds a `CorsLayer` from user-configurable origin patterns.
//!
//! `tile_server.rs` owns *lifecycle* concerns only: configuration ingestion,
//! building the router, applying cross-cutting middlewares (CORS, backpressure,
//! timeouts, panic catching), listening on a socket, graceful shutdown, and
//! a tiny `/status` probe for liveness checks.

use super::{cors, routes, sources};
use crate::config::{Config, TileSourceConfig};
use anyhow::{Result, bail};
use arc_swap::ArcSwap;
use axum::error_handling::HandleErrorLayer;
use axum::http::{StatusCode, header::HeaderName, header::HeaderValue};
use axum::{BoxError, response::IntoResponse};
use axum::{Router, routing::get};
use dashmap::DashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::oneshot};
use tower::{
	ServiceBuilder, buffer::BufferLayer, limit::ConcurrencyLimitLayer, load_shed::LoadShedLayer, timeout::TimeoutLayer,
};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_derive::context;

/// Thin orchestration layer for the VersaTiles HTTP server.
///
/// This type is intentionally small: it stores configuration and composes the
/// router and global middleware stack, but delegates request handling and routing
/// to dedicated modules. The important guarantees are:
/// - **Idempotent start/stop:** starting twice stops the previous instance; stopping twice is a no-op.
/// - **Graceful shutdown:** in-flight requests are allowed to finish (up to a timeout).
/// - **Backpressure by default:** global limits protect the process from overload.
///
/// Typical usage in tests:
/// ```no_run
/// # use versatiles::{config::Config, server::TileServer};
/// # async fn demo(mut server: TileServer) {
/// server.start().await.unwrap();
/// // ... run requests ...
/// server.stop().await; // wait until the listening task has finished
/// # }
/// ```
pub struct TileServer {
	ip: String,
	port: u16,
	/// Tile sources stored in a lock-free concurrent HashMap for dynamic hot-reload.
	/// DashMap provides lock-free reads (serving tiles) with sharded locking for writes (add/remove).
	tile_sources: Arc<DashMap<String, Arc<sources::ServerTileSource>>>,
	/// Static sources stored in a lock-free arc-swapped Vec for dynamic hot-reload.
	/// ArcSwap allows lock-free reads (serving files) and copy-on-write updates (add/remove).
	static_sources: Arc<ArcSwap<Vec<sources::StaticSource>>>,
	/// One-shot channel to signal graceful shutdown to the serving task.
	exit_signal: Option<oneshot::Sender<()>>,
	/// Join handle for the serving task; awaited in `stop()` to ensure shutdown completes.
	join: Option<tokio::task::JoinHandle<()>>,
	/// If true, prefer faster (lower ratio) recompression when negotiating encodings.
	minimal_recompression: bool,
	/// Expose small helper endpoints like `/tiles/index.json` and `/status`.
	disable_api: bool,
	runtime: TilesRuntime,
	/// Configured CORS origins (supports `*`, prefix/suffix wildcard, or `/regex/`).
	cors_allowed_origins: Vec<String>,
	cors_max_age_seconds: u64,
	/// Extra response headers as configured.
	extra_response_headers: Vec<(HeaderName, HeaderValue)>,
}

impl TileServer {
	#[cfg(test)]
	pub fn new_test(ip: &str, port: u16, minimal_recompression: bool, disable_api: bool) -> TileServer {
		let runtime = crate::runtime::create_test_runtime();
		TileServer {
			ip: ip.to_owned(),
			port,
			tile_sources: Arc::new(DashMap::new()),
			static_sources: Arc::new(ArcSwap::from_pointee(Vec::new())),
			exit_signal: None,
			join: None,
			minimal_recompression,
			disable_api,
			runtime,
			cors_allowed_origins: Vec::new(),
			cors_max_age_seconds: 3600,
			extra_response_headers: Vec::new(),
		}
	}

	/// Construct a server from `Config` and a `ContainerRegistry`.
	///
	/// This ingests tile and static sources, applying optional on-the-fly
	/// transforms (e.g., `flip_y`, `swap_xy`) and compression overrides.
	#[context("building tile server from config")]
	pub async fn from_config(config: Config, runtime: TilesRuntime) -> Result<TileServer> {
		let mut parsed_headers: Vec<(HeaderName, HeaderValue)> = Vec::new();
		for (k, v) in &config.extra_response_headers {
			let name =
				HeaderName::from_bytes(k.as_bytes()).map_err(|e| anyhow::anyhow!("invalid header name {k:?}: {e}"))?;
			let value = HeaderValue::from_str(v).map_err(|e| anyhow::anyhow!("invalid header value for {k:?}: {e}"))?;
			parsed_headers.push((name, value));
		}

		let mut server = TileServer {
			ip: config.server.ip.unwrap_or("0.0.0.0".into()),
			port: config.server.port.unwrap_or(8080),
			tile_sources: Arc::new(DashMap::new()),
			static_sources: Arc::new(ArcSwap::from_pointee(Vec::new())),
			exit_signal: None,
			join: None,
			minimal_recompression: config.server.minimal_recompression.unwrap_or(false),
			disable_api: config.server.disable_api.unwrap_or(false),
			runtime,
			cors_allowed_origins: config.cors.allowed_origins.clone(),
			cors_max_age_seconds: config.cors.max_age_seconds.unwrap_or(3600),
			extra_response_headers: parsed_headers,
		};

		for tile_config in &config.tile_sources {
			server.add_tile_source_config(tile_config).await?;
		}

		for static_config in &config.static_sources {
			server
				.add_static_source(
					static_config.src.as_path()?,
					static_config.prefix.as_deref().unwrap_or("/"),
				)
				.await?;
		}

		Ok(server)
	}

	#[context("adding tile source from config: {tile_config:?}")]
	async fn add_tile_source_config(&mut self, tile_config: &TileSourceConfig) -> Result<()> {
		let name = tile_config.name.clone().unwrap_or(tile_config.src.name()?.to_string());

		log::debug!(
			"add source: name='{}', path={:?}",
			tile_config.name.as_deref().unwrap_or("<unnamed>"),
			tile_config.src,
		);

		let reader = self.runtime.get_reader(tile_config.src.clone()).await?;

		self.add_tile_source(name, reader).await
	}

	/// Add a tile source dynamically while server is running.
	///
	/// Returns error if a source with this name already exists or if URL prefix collides.
	/// Can be called before or after `start()` - changes take effect immediately.
	#[context("adding tile source: id='{name}'")]
	pub async fn add_tile_source(&mut self, name: String, reader: Arc<Box<dyn TileSource>>) -> Result<()> {
		log::debug!("add source: id='{name}', source={reader:?}");

		// Create ServerTileSource (validates and wraps reader)
		let source = sources::ServerTileSource::from(reader, &name)?;
		let source_arc = Arc::new(source);

		// Check for ID collision
		if self.tile_sources.contains_key(&name) {
			bail!("tile source '{name}' already exists");
		}

		// Check URL prefix collision with existing sources
		let new_prefix = source_arc.prefix.clone();
		for entry in self.tile_sources.iter() {
			let (other_id, other_source) = entry.pair();
			let other_prefix = &other_source.prefix;
			if other_prefix.starts_with(&new_prefix) || new_prefix.starts_with(other_prefix) {
				bail!(
					"URL prefix collision: new source '{name}' ({new_prefix}) conflicts with existing source '{other_id}' ({other_prefix})"
				);
			}
		}

		// Insert into DashMap (lock-free!)
		self.tile_sources.insert(name.clone(), source_arc);

		log::info!("added tile source: id='{name}', prefix='{new_prefix}'");
		Ok(())
	}

	/// Remove a tile source dynamically while server is running.
	///
	/// Returns true if the source was found and removed, false if not found.
	/// In-flight requests to the removed source will complete successfully
	/// due to Arc reference counting.
	pub fn remove_tile_source(&mut self, name: &str) -> Result<bool> {
		let removed = self.tile_sources.remove(name);

		if removed.is_some() {
			log::info!("removed tile source: id='{name}'");
			Ok(true)
		} else {
			log::debug!("tile source '{name}' not found for removal");
			Ok(false)
		}
	}

	/// Register a static file source mounted at `url_prefix`.
	///
	/// Uses read-copy-update (RCU) for lock-free hot-reload.
	/// Can be called before or after `start()` - changes take effect immediately.
	#[context("adding static source: path={path:?}, url_prefix='{url_prefix}'")]
	pub async fn add_static_source(&mut self, path: &Path, url_prefix: &str) -> Result<()> {
		log::debug!("add static: {path:?}");
		let source = sources::StaticSource::new(path, url_prefix)?;
		self.static_sources.rcu(|old| {
			let mut new = (**old).clone();
			new.push(source.clone());
			new
		});
		log::info!("added static source: path={path:?}, url_prefix='{url_prefix}'");
		Ok(())
	}

	/// Remove a static source by URL prefix.
	///
	/// Returns true if a source was removed, false if prefix not found.
	/// In-flight requests to the removed source will complete successfully.
	/// Uses read-copy-update (RCU) for lock-free hot-reload.
	pub fn remove_static_source(&mut self, url_prefix: &str) -> Result<bool> {
		let target_prefix = crate::server::Url::from(url_prefix).to_dir();

		let initial_len = self.static_sources.load().len();
		self.static_sources.rcu(|old| {
			let new: Vec<_> = old
				.iter()
				.filter(|source| source.get_prefix() != &target_prefix)
				.cloned()
				.collect();
			new
		});
		let was_removed = self.static_sources.load().len() < initial_len;

		if was_removed {
			log::info!("removed static source: url_prefix='{url_prefix}'");
		} else {
			log::debug!("static source '{url_prefix}' not found for removal");
		}

		Ok(was_removed)
	}

	/// Start listening and serving requests.
	///
	/// - Idempotent: if already running, the previous instance is stopped first.
	/// - Builds the router (`routes`), applies CORS and global protection layers,
	///   then spawns `axum::serve(...)` with graceful shutdown support.
	#[context("starting tile server")]
	pub async fn start(&mut self) -> Result<()> {
		// If already running, stop first to avoid port conflicts and leaked tasks.
		if self.exit_signal.is_some() || self.join.is_some() {
			self.stop().await;
		}

		log::info!("starting server");

		// Build the router
		let mut router = Router::new().route("/status", get(|| async { "ready!" }));
		router = self.add_tile_sources_to_app(router);
		if !self.disable_api {
			router = self.add_api_to_app(router).await?;
		}
		router = self.add_static_sources_to_app(router);

		let cors_layer = cors::build_cors_layer(&self.cors_allowed_origins, self.cors_max_age_seconds)?;
		router = router.layer(ServiceBuilder::new().layer(cors_layer));

		// Apply any extra response headers from configuration (overriding existing values).
		for (name, value) in self.extra_response_headers.iter().cloned() {
			router = router.layer(SetResponseHeaderLayer::overriding(name, value));
		}

		// --- Global backpressure & protection layers ---
		// The order of layers matters. From innermost to outermost:
		//   LoadShed → ConcurrencyLimit → Buffer → Timeout → CatchPanic → HandleError
		// We apply `HandleErrorLayer` outermost so Axum observes an `Infallible` error type.
		let global_concurrency = 256usize; // tune based on CPU and workload
		let global_buffer = 512usize; // bounded queue in front of the service
		let request_timeout = std::time::Duration::from_secs(15); // hard per-request cap

		let overload_handler = HandleErrorLayer::new(|_err: BoxError| async move {
			// Map timeouts, loadshed, and buffer-closed errors to a clear 503.
			// 503 is cache-aware and plays well with upstream retries; 429 is reserved for per-client rate limits.
			let mut resp = (StatusCode::SERVICE_UNAVAILABLE, "Service overloaded, try later").into_response();
			resp.headers_mut().insert("Retry-After", "2".parse().unwrap());
			Ok::<_, std::convert::Infallible>(resp)
		});

		let protection = ServiceBuilder::new()
			// Handle all tower errors at the very outside so Router sees Infallible.
			.layer(overload_handler)
			// Don't let panics kill the process.
			.layer(CatchPanicLayer::new())
			// Hard cap per-request wall time.
			.layer(TimeoutLayer::new(request_timeout))
			// Bounded queue in front of the service.
			.layer(BufferLayer::new(global_buffer))
			// Cap in-flight work.
			.layer(ConcurrencyLimitLayer::new(global_concurrency))
			// If saturated, fail fast.
			.layer(LoadShedLayer::new());

		router = router.layer(protection);

		let addr = format!("{}:{}", self.ip, self.port);
		log::info!("server binding on {addr}");

		let listener = TcpListener::bind(&addr).await?;
		// If we asked for an ephemeral port (0), record the actual assigned port for test URLs.
		if self.port == 0 {
			self.port = listener.local_addr()?.port();
		}
		let (tx, rx) = oneshot::channel::<()>();

		// Spawn the server and keep a handle so we can await it on shutdown.
		let handle = tokio::spawn(async move {
			if let Err(err) = axum::serve(listener, router.into_make_service())
				.with_graceful_shutdown(async {
					rx.await.ok();
				})
				.await
			{
				// The task boundary is a good place to log; we can't bubble this up after spawn.
				log::error!("server task exited with error: {err}");
			}
		});

		self.exit_signal = Some(tx);
		self.join = Some(handle);

		Ok(())
	}

	/// Trigger graceful shutdown and wait for the server task to finish (with timeout).
	///
	/// Idempotent: if the server is not running, this returns immediately.
	pub async fn stop(&mut self) {
		// If not running, do nothing (idempotent).
		if self.exit_signal.is_none() && self.join.is_none() {
			return;
		}

		log::info!("stopping server");

		// Signal graceful shutdown.
		if let Some(tx) = self.exit_signal.take() {
			let _ = tx.send(());
		}

		// Await the server task to finish, but don't hang forever.
		if let Some(handle) = self.join.take() {
			match tokio::time::timeout(std::time::Duration::from_secs(10), handle).await {
				Ok(join_result) => {
					if let Err(join_err) = join_result {
						log::warn!("server task join error: {join_err}");
					}
				}
				Err(_) => {
					log::warn!("server task did not shutdown within timeout; continuing");
				}
			}
		}
	}

	/// Get the port the server is listening on (or will listen on).
	///
	/// If the server was started with port 0, this returns the actual ephemeral port
	/// assigned after binding.
	pub fn get_port(&self) -> u16 {
		self.port
	}

	/// Helper: delegate to `routes::add_tile_sources_to_app` to attach tile endpoints.
	fn add_tile_sources_to_app(&self, app: Router) -> Router {
		routes::add_tile_sources_to_app(app, Arc::clone(&self.tile_sources), self.minimal_recompression)
	}

	/// Helper: delegate to `routes::add_static_sources_to_app` to attach static endpoints.
	fn add_static_sources_to_app(&self, app: Router) -> Router {
		routes::add_static_sources_to_app(app, Arc::clone(&self.static_sources), self.minimal_recompression)
	}

	/// Helper: delegate to `routes::add_api_to_app` to attach small JSON API endpoints.
	#[context("adding API routes to app")]
	async fn add_api_to_app(&self, app: Router) -> Result<Router> {
		routes::add_api_to_app(app, Arc::clone(&self.tile_sources)).await
	}

	pub fn get_url_mapping(&self) -> Vec<(super::Url, String)> {
		let mut result = Vec::new();
		for entry in self.tile_sources.iter() {
			let tile_source = entry.value();
			let source_name = tile_source.get_source_name();
			result.push((tile_source.prefix.clone(), source_name));
		}
		result
	}
}

/// Integration tests for server lifecycle, routing, and content negotiation.
/// These spin up a real TCP listener on localhost ports (see port numbers in cases).
#[cfg(test)]
mod tests {
	use super::*;
	use axum::http::{HeaderMap, HeaderValue, header};
	use regex::Regex;
	use reqwest::Client;
	use rstest::rstest;
	use std::sync::Arc;
	use versatiles_container::{MockReader, MockReaderProfile as MRP, TileSourceMetadata, Traversal};
	use versatiles_core::{TileBBoxPyramid, TileCompression as TC, TileFormat as TF};

	const IP: &str = "127.0.0.1";

	#[tokio::test]
	async fn server() -> Result<()> {
		async fn get(path: &str) -> String {
			reqwest::get(format!("http://{IP}:50001/{path}"))
				.await
				.expect("should have made a get request")
				.text()
				.await
				.expect("should have returned text")
		}

		let mut server = TileServer::new_test(IP, 50001, true, false);

		let reader = Arc::new(MockReader::new_mock_profile(MRP::Pbf)?.boxed());
		server.add_tile_source("cheese".to_string(), reader).await?;

		server.start().await?;

		assert_eq!(get("tiles/cheese/brum.json").await, "Not Found");

		let meta = "{\"bounds\":[-180,-85.051129,180,85.051129],\"maxzoom\":6,\"minzoom\":2,\"tile_format\":\"vnd.mapbox-vector-tile\",\"tile_schema\":\"other\",\"tile_type\":\"vector\",\"tilejson\":\"3.0.0\",\"tiles\":[\"/tiles/cheese/{z}/{x}/{y}\"],\"type\":\"dummy\"}";
		assert_eq!(get("tiles/cheese/meta.json").await, meta);
		assert_eq!(get("tiles/cheese/tiles.json").await, meta);
		assert_eq!(&get("tiles/cheese/3/4/5").await[0..9], "\u{1a}4\n\u{5}ocean");
		assert_eq!(get("tiles/index.json").await, "[\"cheese\"]");
		assert_eq!(get("status").await, "ready!");

		server.stop().await;

		Ok(())
	}

	#[tokio::test]
	#[should_panic(expected = "already exists")]
	async fn same_prefix_twice() {
		let mut server = TileServer::new_test(IP, 0, true, false);

		let reader = Arc::new(MockReader::new_mock_profile(MRP::Png).unwrap().boxed());
		server.add_tile_source("cheese".to_string(), reader).await.unwrap();

		let reader = Arc::new(MockReader::new_mock_profile(MRP::Pbf).unwrap().boxed());
		server.add_tile_source("cheese".to_string(), reader).await.unwrap();
	}

	#[tokio::test]
	async fn tile_server_new() {
		let mut server = TileServer::new_test(IP, 50003, true, false);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50003);
		assert_eq!(server.tile_sources.len(), 0);
		assert_eq!(server.static_sources.load().len(), 0);
		assert!(server.exit_signal.is_none());
		server.start().await.unwrap();
		server.stop().await; // No assertion here as it's void
	}

	#[tokio::test]
	async fn tile_server_add_tile_source() {
		let mut server = TileServer::new_test(IP, 0, true, false);
		assert_eq!(server.ip, IP);

		let reader = Arc::new(MockReader::new_mock_profile(MRP::Pbf).unwrap().boxed());
		server.add_tile_source("cheese".to_string(), reader).await.unwrap();

		assert_eq!(server.tile_sources.len(), 1);
		assert!(server.tile_sources.contains_key("cheese"));
		assert_eq!(server.tile_sources.get("cheese").unwrap().prefix.str, "/tiles/cheese/");
	}

	#[tokio::test]
	async fn tile_server_iter_url_mapping() {
		let mut server = TileServer::new_test(IP, 0, true, false);
		assert_eq!(server.ip, IP);

		let reader = Arc::new(MockReader::new_mock_profile(MRP::Pbf).unwrap().boxed());
		server.add_tile_source("cheese".to_string(), reader).await.unwrap();

		assert_eq!(
			server.get_url_mapping(),
			vec![(
				crate::server::Url::from("/tiles/cheese/"),
				"container 'dummy' ('dummy')".to_string()
			)]
		);
	}

	#[rstest]
	#[case(TF::MVT, TC::Gzip, "br", "br", "vnd.mapbox-vector-tile")]
	#[case(TF::MVT, TC::Gzip, "gzip", "gzip", "vnd.mapbox-vector-tile")]
	#[case(TF::MVT, TC::Brotli, "br", "br", "vnd.mapbox-vector-tile")]
	#[case(TF::MVT, TC::Brotli, "gzip", "gzip", "vnd.mapbox-vector-tile")]
	#[case(TF::MVT, TC::Uncompressed, "", "", "vnd.mapbox-vector-tile")]
	#[case(TF::PNG, TC::Gzip, "br", "", "image/png")]
	#[case(TF::WEBP, TC::Brotli, "gzip", "", "image/webp")]
	#[tokio::test]
	async fn serve_tile_variants(
		#[case] format: TF,
		#[case] compression: TC,
		#[case] accept_encoding: &str,
		#[case] expect_content_encoding: &str,
		#[case] expect_mime_contains: &str,
	) {
		async fn fetch_raw(url: &str, accept_encoding: &str) -> reqwest::Response {
			let client = Client::builder()
				.gzip(false)
				.brotli(false)
				.deflate(false)
				.build()
				.unwrap();
			let mut headers = HeaderMap::new();
			if !accept_encoding.is_empty() {
				headers.insert("accept-encoding", HeaderValue::from_str(accept_encoding).unwrap());
			}
			client.get(url).headers(headers).send().await.expect("http get")
		}

		let mut server = TileServer::new_test(IP, 0, true, false);

		let parameters = TileSourceMetadata::new(format, compression, TileBBoxPyramid::new_full(8), Traversal::ANY);
		let reader = Arc::new(MockReader::new_mock(parameters).unwrap().boxed());
		server.add_tile_source("cheese".to_string(), reader).await.unwrap();
		server.start().await.unwrap();

		let url = format!("http://{IP}:{}/tiles/cheese/3/3/3", server.port);
		let resp = fetch_raw(&url, accept_encoding).await;
		assert_eq!(resp.status(), 200);

		let headers = resp.headers();
		let ct = headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
		assert_eq!(
			ct, expect_mime_contains,
			"unexpected content-type '{ct}', expected to be '{expect_mime_contains}'"
		);

		let content_encoding = headers
			.get(header::CONTENT_ENCODING)
			.map_or("", |v| v.to_str().unwrap());
		assert_eq!(
			content_encoding, expect_content_encoding,
			"unexpected content-encoding '{content_encoding}', expected to be '{expect_content_encoding}'"
		);

		let bytes = resp.bytes().await.expect("bytes");
		assert!(!bytes.is_empty(), "empty body");

		server.stop().await;
	}

	#[tokio::test]
	async fn static_sources_serve_files() -> Result<()> {
		let mut server = TileServer::new_test(IP, 0, true, false); // use ephemeral port to avoid Windows ACL/ephemeral conflicts

		// Mount the provided test archive at root.
		let static_path = Path::new("../testdata/static.tar.br");
		server
			.add_static_source(static_path, "/")
			.await
			.expect("add static source");
		server.start().await.expect("start server");
		let port = server.port;

		let client = Arc::new(Client::builder().build().unwrap());

		let test_request = |url: &str, expected_status: u16, expected_content_type: &str, expected_body: &str| {
			let client = client.clone();
			let url = format!("http://{IP}:{port}{url}");
			let expected_content_type = format!("{expected_content_type}; charset=utf-8");
			let expected_body = expected_body.to_string();
			async move {
				let response = client.get(&url).send().await.unwrap();
				assert_eq!(
					response.status().as_u16(),
					expected_status,
					"Wrong status for URL: {url}"
				);

				let content_type = response
					.headers()
					.get(header::CONTENT_TYPE)
					.unwrap()
					.to_str()
					.unwrap()
					.to_string();
				assert_eq!(content_type, expected_content_type, "Wrong content-type for URL: {url}");

				let body = response.text().await.unwrap();
				let body = Regex::new(r"\s+").unwrap().replace_all(&body, " ").trim().to_string();
				let body = body[0..9].to_string();
				assert_eq!(body, expected_body, "Wrong body start for URL: {url}");
			}
		};

		test_request("", 200, "text/html", "<html> <h").await;
		test_request("/", 200, "text/html", "<html> <h").await;
		test_request("/index.html", 200, "text/html", "<html> <h").await;
		test_request("/style.css", 200, "text/css", "body { ma").await;
		test_request("/missing.txt", 404, "text/plain", "Not Found").await;

		server.stop().await;
		Ok(())
	}

	#[tokio::test]
	async fn extra_response_headers_are_applied() -> Result<()> {
		// Use ephemeral port to avoid conflicts on CI/Windows.
		let mut server = TileServer::new_test(IP, 0, true, false);

		// Inject extra headers directly for the test.
		server.extra_response_headers = vec![(HeaderName::from_static("x-test-header"), HeaderValue::from_static("ok"))];

		server.start().await?;
		let port = server.port;

		let url = format!("http://{IP}:{port}/status");
		let resp = reqwest::get(&url).await.unwrap();
		assert_eq!(resp.status(), 200);
		let headers = resp.headers();

		assert_eq!(
			headers.get("x-test-header").and_then(|v| v.to_str().ok()),
			Some("ok"),
			"expected custom header to be present on /status"
		);

		server.stop().await;
		Ok(())
	}

	#[tokio::test]
	async fn extra_response_headers_multiple_and_values() -> Result<()> {
		let mut server = TileServer::new_test(IP, 0, true, false);

		server.extra_response_headers = vec![
			(HeaderName::from_static("x-foo"), HeaderValue::from_static("alpha")),
			(HeaderName::from_static("x-bar"), HeaderValue::from_static("beta")),
		];

		server.start().await?;
		let port = server.port;

		let client = Client::builder().build().unwrap();
		let url = format!("http://{IP}:{port}/status");
		let resp = client.get(&url).send().await.unwrap();
		assert_eq!(resp.status(), 200);
		let headers = resp.headers();

		assert_eq!(headers.get("x-foo").and_then(|v| v.to_str().ok()), Some("alpha"));
		assert_eq!(headers.get("x-bar").and_then(|v| v.to_str().ok()), Some("beta"));

		server.stop().await;
		Ok(())
	}

	#[tokio::test]
	async fn remove_static_source_returns_true_when_found() -> Result<()> {
		let mut server = TileServer::new_test(IP, 0, true, false);

		let static_path = Path::new("../testdata/static.tar.br");
		server.add_static_source(static_path, "/static").await?;

		assert_eq!(server.static_sources.load().len(), 1);

		let removed = server.remove_static_source("/static")?;

		assert!(removed, "should return true when source is removed");
		assert_eq!(server.static_sources.load().len(), 0);
		Ok(())
	}

	#[tokio::test]
	async fn remove_static_source_returns_false_when_not_found() -> Result<()> {
		let mut server = TileServer::new_test(IP, 0, true, false);

		let removed = server.remove_static_source("/nonexistent")?;

		assert!(!removed, "should return false when source not found");
		Ok(())
	}

	#[tokio::test]
	async fn remove_static_source_with_existing_sources() -> Result<()> {
		let mut server = TileServer::new_test(IP, 0, true, false);

		let static_path = Path::new("../testdata/static.tar.br");
		server.add_static_source(static_path, "/first").await?;
		server.add_static_source(static_path, "/second").await?;

		assert_eq!(server.static_sources.load().len(), 2);

		// Remove only the first one
		let removed = server.remove_static_source("/first")?;

		assert!(removed);
		assert_eq!(server.static_sources.load().len(), 1);

		// Verify the second one still exists
		let sources = server.static_sources.load();
		assert_eq!(sources[0].get_prefix().str, "/second/");
		Ok(())
	}

	#[tokio::test]
	async fn remove_static_source_serves_404_after_removal() -> Result<()> {
		let mut server = TileServer::new_test(IP, 0, true, false);

		let static_path = Path::new("../testdata/static.tar.br");
		server.add_static_source(static_path, "/static").await?;
		server.start().await?;
		let port = server.port;

		let client = Client::builder().build().unwrap();

		// Verify source is accessible before removal
		let resp = client.get(format!("http://{IP}:{port}/static/")).send().await?;
		assert_eq!(resp.status().as_u16(), 200);

		// Remove the source
		let removed = server.remove_static_source("/static")?;
		assert!(removed);

		// Verify source is no longer accessible
		let resp = client.get(format!("http://{IP}:{port}/static/")).send().await?;
		assert_eq!(resp.status().as_u16(), 404);

		server.stop().await;
		Ok(())
	}
}
