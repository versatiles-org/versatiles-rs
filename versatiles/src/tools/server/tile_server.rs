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

use super::{cors, routes, sources, utils::Url};
use anyhow::{Result, bail};
use axum::error_handling::HandleErrorLayer;
use axum::http::StatusCode;
use axum::{BoxError, response::IntoResponse};
use axum::{Router, routing::get};
use std::path::Path;
use tokio::{net::TcpListener, sync::oneshot};
use tower::{
	ServiceBuilder, buffer::BufferLayer, limit::ConcurrencyLimitLayer, load_shed::LoadShedLayer, timeout::TimeoutLayer,
};
use tower_http::catch_panic::CatchPanicLayer;
#[cfg(test)]
use versatiles::get_registry;
use versatiles::{Config, TileSourceConfig};
#[cfg(test)]
use versatiles_container::ProcessingConfig;
use versatiles_container::{ContainerRegistry, TilesConvertReader, TilesConverterParameters, TilesReaderTrait};
use versatiles_core::TileCompression;
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
/// # use versatiles::{Config};
/// # async fn demo(mut server: TileServer) {
/// server.start().await.unwrap();
/// // ... run requests ...
/// server.stop().await; // wait until the listening task has finished
/// # }
/// ```
pub struct TileServer {
	ip: String,
	port: u16,
	tile_sources: Vec<sources::TileSource>,
	static_sources: Vec<sources::StaticSource>,
	/// One-shot channel to signal graceful shutdown to the serving task.
	exit_signal: Option<oneshot::Sender<()>>,
	/// Join handle for the serving task; awaited in `stop()` to ensure shutdown completes.
	join: Option<tokio::task::JoinHandle<()>>,
	/// If true, prefer faster (lower ratio) recompression when negotiating encodings.
	minimal_recompression: bool,
	/// Expose small helper endpoints like `/tiles/index.json` and `/status`.
	use_api: bool,
	registry: ContainerRegistry,
	/// Configured CORS origins (supports `*`, prefix/suffix wildcard, or `/regex/`).
	cors_allowed_origins: Vec<String>,
}

impl TileServer {
	#[cfg(test)]
	pub fn new_test(ip: &str, port: u16, minimal_recompression: bool, use_api: bool) -> TileServer {
		TileServer {
			ip: ip.to_owned(),
			port,
			tile_sources: Vec::new(),
			static_sources: Vec::new(),
			exit_signal: None,
			join: None,
			minimal_recompression,
			use_api,
			registry: get_registry(ProcessingConfig::default()),
			cors_allowed_origins: Vec::new(),
		}
	}

	/// Construct a server from `Config` and a `ContainerRegistry`.
	///
	/// This ingests tile and static sources, applying optional on-the-fly
	/// transforms (e.g., `flip_y`, `swap_xy`) and compression overrides.
	pub async fn from_config(config: Config, registry: ContainerRegistry) -> Result<TileServer> {
		let mut server = TileServer {
			ip: config.server.ip.unwrap_or("0.0.0.0".into()),
			port: config.server.port.unwrap_or(8080),
			tile_sources: Vec::new(),
			static_sources: Vec::new(),
			exit_signal: None,
			join: None,
			minimal_recompression: config.server.minimal_recompression.unwrap_or(false),
			use_api: !config.server.disable_api.unwrap_or(false),
			registry,
			cors_allowed_origins: config.cors.allowed_origins.clone(),
		};

		for tile_config in config.tile_sources.iter() {
			server.add_tile_source_config(tile_config).await?;
		}

		for static_config in config.static_sources.iter() {
			server.add_static_source(
				static_config.path.as_path()?,
				static_config.url_prefix.as_deref().unwrap_or("/"),
			)?;
		}

		Ok(server)
	}

	#[context("adding tile source from config: {tile_config:?}")]
	async fn add_tile_source_config(&mut self, tile_config: &TileSourceConfig) -> Result<()> {
		let name = tile_config.name.clone().unwrap_or(tile_config.path.name()?);

		log::info!(
			"add source: name='{}', path={:?}",
			tile_config.name.as_deref().unwrap_or("<unnamed>"),
			tile_config.path,
		);

		let mut reader = self.registry.get_reader(&tile_config.path).await?;

		if let Some(comp_str) = tile_config.override_compression.as_ref() {
			reader.override_compression(TileCompression::try_from(comp_str.as_str())?);
		}

		let flip_y = tile_config.flip_y.unwrap_or(false);
		let swap_xy = tile_config.swap_xy.unwrap_or(false);

		if flip_y || swap_xy {
			let cp = TilesConverterParameters {
				flip_y,
				swap_xy,
				..Default::default()
			};
			reader = TilesConvertReader::new_from_reader(reader, cp)?.boxed();
		}

		self.add_tile_source(&name, reader)
	}

	/// Register a tile source under `/tiles/<name>/...`.
	///
	/// Fails if the URL prefix collides (as a prefix) with an existing source.
	pub fn add_tile_source(&mut self, name: &str, reader: Box<dyn TilesReaderTrait>) -> Result<()> {
		log::info!("add source: id='{name}', source={reader:?}");

		let source = sources::TileSource::from(reader, name)?;
		let url_prefix = &source.prefix;

		for other_tile_source in self.tile_sources.iter() {
			let other_prefix = &other_tile_source.prefix;
			if other_prefix.starts_with(url_prefix) || url_prefix.starts_with(other_prefix) {
				bail!("multiple sources with the prefix '{url_prefix}' and '{other_prefix}' are defined");
			};
		}

		self.tile_sources.push(source);

		Ok(())
	}

	/// Register a static file source mounted at `url_prefix`.
	pub fn add_static_source(&mut self, path: &Path, url_prefix: &str) -> Result<()> {
		log::info!("add static: {path:?}");
		self
			.static_sources
			.push(sources::StaticSource::new(path, Url::new(url_prefix))?);
		Ok(())
	}

	/// Start listening and serving requests.
	///
	/// - Idempotent: if already running, the previous instance is stopped first.
	/// - Builds the router (`routes`), applies CORS and global protection layers,
	///   then spawns `axum::serve(...)` with graceful shutdown support.
	pub async fn start(&mut self) -> Result<()> {
		// If already running, stop first to avoid port conflicts and leaked tasks.
		if self.exit_signal.is_some() || self.join.is_some() {
			self.stop().await;
		}

		log::info!("starting server");

		// Build the router
		let mut router = Router::new().route("/status", get(|| async { "ready!" }));
		router = self.add_tile_sources_to_app(router);
		if self.use_api {
			router = self.add_api_to_app(router).await?;
		}
		router = self.add_static_sources_to_app(router);

		let cors_layer = cors::build_cors_layer(&self.cors_allowed_origins)?;
		router = router.layer(ServiceBuilder::new().layer(cors_layer));

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

	/// Helper: delegate to `routes::add_tile_sources_to_app` to attach tile endpoints.
	fn add_tile_sources_to_app(&self, app: Router) -> Router {
		routes::add_tile_sources_to_app(app, &self.tile_sources, self.minimal_recompression)
	}

	/// Helper: delegate to `routes::add_static_sources_to_app` to attach static endpoints.
	fn add_static_sources_to_app(&self, app: Router) -> Router {
		routes::add_static_sources_to_app(app, &self.static_sources, self.minimal_recompression)
	}

	/// Helper: delegate to `routes::add_api_to_app` to attach small JSON API endpoints.
	async fn add_api_to_app(&self, app: Router) -> Result<Router> {
		routes::add_api_to_app(app, &self.tile_sources).await
	}

	pub async fn get_url_mapping(&self) -> Vec<(String, String)> {
		let mut result = Vec::new();
		for tile_source in self.tile_sources.iter() {
			let id = tile_source.get_source_name().await;
			result.push((tile_source.prefix.as_string(), id.to_owned()))
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
	use versatiles_container::{MockTilesReader, MockTilesReaderProfile as MTRP};
	use versatiles_core::{TileBBoxPyramid, TileCompression as TC, TileFormat as TF, TilesReaderParameters};

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

		let mut server = TileServer::new_test(IP, 50001, true, true);

		let reader = MockTilesReader::new_mock_profile(MTRP::Pbf)?.boxed();
		server.add_tile_source("cheese", reader)?;

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
	#[should_panic]
	async fn same_prefix_twice() {
		let mut server = TileServer::new_test(IP, 50002, true, true);

		let reader = MockTilesReader::new_mock_profile(MTRP::Png).unwrap().boxed();
		server.add_tile_source("cheese", reader).unwrap();

		let reader = MockTilesReader::new_mock_profile(MTRP::Pbf).unwrap().boxed();
		server.add_tile_source("cheese", reader).unwrap();
	}

	#[tokio::test]
	async fn tile_server_new() {
		let mut server = TileServer::new_test(IP, 50003, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50003);
		assert_eq!(server.tile_sources.len(), 0);
		assert_eq!(server.static_sources.len(), 0);
		assert!(server.exit_signal.is_none());
		server.start().await.unwrap();
		server.stop().await; // No assertion here as it's void
	}

	#[test]
	fn tile_server_add_tile_source() {
		let mut server = TileServer::new_test(IP, 50004, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50004);

		let reader = MockTilesReader::new_mock_profile(MTRP::Pbf).unwrap().boxed();
		server.add_tile_source("cheese", reader).unwrap();

		assert_eq!(server.tile_sources.len(), 1);
		assert_eq!(server.tile_sources[0].prefix.str, "/tiles/cheese/");
	}

	#[tokio::test]
	async fn tile_server_iter_url_mapping() {
		let mut server = TileServer::new_test(IP, 50005, true, true);
		assert_eq!(server.ip, IP);
		assert_eq!(server.port, 50005);

		let reader = MockTilesReader::new_mock_profile(MTRP::Pbf).unwrap().boxed();
		server.add_tile_source("cheese", reader).unwrap();

		let mappings: Vec<(String, String)> = server.get_url_mapping().await;
		assert_eq!(mappings.len(), 1);
		assert_eq!(mappings[0].0, "/tiles/cheese/");
		assert_eq!(mappings[0].1, "dummy_name");
	}

	#[rstest]
	#[case(50110, TF::MVT, TC::Gzip, "br", "br", "vnd.mapbox-vector-tile")]
	#[case(50111, TF::MVT, TC::Gzip, "gzip", "gzip", "vnd.mapbox-vector-tile")]
	#[case(50112, TF::MVT, TC::Brotli, "br", "br", "vnd.mapbox-vector-tile")]
	#[case(50113, TF::MVT, TC::Brotli, "gzip", "gzip", "vnd.mapbox-vector-tile")]
	#[case(50114, TF::MVT, TC::Uncompressed, "", "", "vnd.mapbox-vector-tile")]
	#[case(50115, TF::PNG, TC::Gzip, "br", "", "image/png")]
	#[case(50116, TF::WEBP, TC::Brotli, "gzip", "", "image/webp")]
	#[tokio::test]
	async fn serve_tile_variants(
		#[case] port: u16,
		#[case] format: TF,
		#[case] compression: TileCompression,
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

		let mut server = TileServer::new_test(IP, port, true, true);

		let parameters = TilesReaderParameters::new(format, compression, TileBBoxPyramid::new_full(8));
		let reader = MockTilesReader::new_mock(parameters).unwrap().boxed();
		server.add_tile_source("cheese", reader).unwrap();
		server.start().await.unwrap();

		let url = format!("http://{IP}:{port}/tiles/cheese/3/3/3");
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
			.map(|v| v.to_str().unwrap())
			.unwrap_or("");
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
		let port = 50006;
		let mut server = TileServer::new_test(IP, port, true, true);

		// Mount the provided test archive at root.
		let static_path = Path::new("../testdata/static.tar.br");
		server.add_static_source(static_path, "/").expect("add static source");
		server.start().await.expect("start server");

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
}
