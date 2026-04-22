//! # TileJSON remote tile source
//!
//! This operation fetches tiles from a remote tile server using a TileJSON endpoint.
//! It downloads the TileJSON metadata, extracts the tile URL template, and serves
//! individual tiles by fetching them from the server on demand.
//!
//! ## Examples
//!
//! ```text
//! from_tilejson url="https://example.com/tiles.json"
//! from_tilejson url="https://example.com/tiles.json" max_retries=5 max_concurrent_requests=64
//! ```

use crate::{PipelineFactory, operations::read::traits::ReadTileSource, vpl::VPLNode};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime, Traversal};
use versatiles_core::{
	Blob, GeoBBox, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream,
};
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads tiles from a remote tile server via a TileJSON endpoint.
/// The TileJSON is fetched from the given URL, and tiles are loaded individually
/// using the URL template from the TileJSON `tiles` array.
struct Args {
	/// The URL of the TileJSON endpoint.
	/// For example: `url="https://example.com/tiles.json"`.
	url: String,
	/// Maximum number of retries per tile request (default: 3).
	max_retries: Option<u16>,
	/// Maximum number of concurrent tile requests (default: io_bound concurrency limit).
	max_concurrent_requests: Option<u16>,
}

struct Operation {
	client: reqwest::Client,
	tile_url_template: String,
	tile_format: TileFormat,
	max_retries: u32,
	max_concurrent_requests: usize,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	url: String,
	runtime: TilesRuntime,
}

impl std::fmt::Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Operation")
			.field("url", &self.url)
			.field("tile_format", &self.tile_format)
			.field("max_retries", &self.max_retries)
			.field("max_concurrent_requests", &self.max_concurrent_requests)
			.finish_non_exhaustive()
	}
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_tilejson operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		let max_retries = u32::from(args.max_retries.unwrap_or(3));
		let max_concurrent_requests = args.max_concurrent_requests.unwrap_or(4) as usize;

		let client = reqwest::Client::builder()
			.tcp_keepalive(Duration::from_secs(600))
			.use_rustls_tls()
			.build()?;

		// Fetch TileJSON
		let response = client.get(&args.url).send().await?;
		if !response.status().is_success() {
			bail!(
				"Failed to fetch TileJSON from '{}': HTTP {}",
				args.url,
				response.status()
			);
		}
		let body = response.text().await?;
		let tilejson = TileJSON::try_from(body.as_str())?;

		// Extract tile URL template
		let tile_url_template = extract_tile_url(&tilejson)?;

		// Detect tile format
		let tile_format = detect_tile_format(&tilejson, &tile_url_template)?;

		// Build tile pyramid from TileJSON bounds/zoom
		let min_zoom = tilejson.zoom_min().unwrap_or(0);
		let max_zoom = tilejson.zoom_max().unwrap_or(22);
		let geo_bbox = tilejson.bounds.unwrap_or_else(|| {
			GeoBBox::new(-180.0, -85.05112878, 180.0, 85.05112878).expect("valid web-mercator bounds literal")
		});
		let tile_pyramid = TilePyramid::from_geo_bbox(min_zoom, max_zoom, &geo_bbox)?;

		let metadata = TileSourceMetadata::new(
			tile_format,
			TileCompression::Uncompressed,
			Traversal::new_any(),
			Some(tile_pyramid),
		);

		let mut result_tilejson = tilejson.clone();
		metadata.update_tilejson(&mut result_tilejson);

		Ok(Box::new(Self {
			client,
			tile_url_template,
			tile_format,
			max_retries,
			max_concurrent_requests,
			metadata,
			tilejson: result_tilejson,
			url: args.url,
			runtime: factory.runtime(),
		}) as Box<dyn TileSource>)
	}
}

fn extract_tile_url(tilejson: &TileJSON) -> Result<String> {
	let obj = tilejson.as_object();
	let tiles_value = obj
		.get("tiles")
		.ok_or_else(|| anyhow!("TileJSON is missing 'tiles' array"))?;
	let tiles_array = tiles_value.as_array()?;
	let tiles = tiles_array.to_string_vec()?;
	let first = tiles
		.into_iter()
		.next()
		.ok_or_else(|| anyhow!("TileJSON 'tiles' array is empty"))?;
	Ok(first)
}

fn detect_tile_format(tilejson: &TileJSON, tile_url_template: &str) -> Result<TileFormat> {
	// First try TileJSON tile_format field
	if let Some(format) = tilejson.tile_format {
		return Ok(format);
	}

	// Then try URL extension
	let mut url = tile_url_template
		.replace("{z}", "0")
		.replace("{x}", "0")
		.replace("{y}", "0");
	if let Some(format) = TileFormat::from_filename(&mut url) {
		return Ok(format);
	}

	bail!("Cannot detect tile format from TileJSON or URL template '{tile_url_template}'")
}

fn build_tile_url(template: &str, coord: &TileCoord) -> String {
	template
		.replace("{z}", &coord.level.to_string())
		.replace("{x}", &coord.x.to_string())
		.replace("{y}", &coord.y.to_string())
}

fn is_retryable_error(err: &reqwest::Error) -> bool {
	err.is_connect() || err.is_timeout() || err.is_body()
}

async fn fetch_tile(
	client: reqwest::Client,
	template: String,
	coord: TileCoord,
	tile_format: TileFormat,
	max_retries: u32,
	runtime: Option<TilesRuntime>,
) -> Option<(TileCoord, Tile)> {
	let url = build_tile_url(&template, &coord);

	for attempt in 0..=max_retries {
		if attempt > 0 {
			let backoff = Duration::from_secs(1 << (attempt - 1));
			log::warn!("retry attempt {attempt}/{max_retries} fetching tile {coord:?} from '{url}', waiting {backoff:?}");
			sleep(backoff).await;
		}

		let response = match client.get(&url).send().await {
			Ok(r) => r,
			Err(e) if is_retryable_error(&e) && attempt < max_retries => {
				log::warn!("retryable error fetching tile {coord:?}: {e}");
				continue;
			}
			Err(e) => {
				if let Some(rt) = &runtime {
					rt.record_error(
						&format!("from_tilejson tile {coord:?}"),
						&anyhow!("HTTP GET '{url}' failed: {e}"),
					);
				} else {
					log::error!("failed to fetch tile {coord:?} from '{url}': {e}");
				}
				return None;
			}
		};

		if response.status() == reqwest::StatusCode::NOT_FOUND {
			return None;
		}

		if !response.status().is_success() {
			if attempt < max_retries {
				log::warn!(
					"HTTP {} fetching tile {coord:?} from '{url}', retrying",
					response.status()
				);
				continue;
			}
			let status = response.status();
			if let Some(rt) = &runtime {
				rt.record_error(
					&format!("from_tilejson tile {coord:?}"),
					&anyhow!("HTTP GET '{url}' returned status {status}"),
				);
			} else {
				log::error!("failed to fetch tile {coord:?} from '{url}': HTTP {status}");
			}
			return None;
		}

		let bytes = match response.bytes().await {
			Ok(b) => b,
			Err(e) if is_retryable_error(&e) && attempt < max_retries => {
				log::warn!("retryable error reading tile {coord:?} body: {e}");
				continue;
			}
			Err(e) => {
				if let Some(rt) = &runtime {
					rt.record_error(
						&format!("from_tilejson tile {coord:?}"),
						&anyhow!("reading response body from '{url}' failed: {e}"),
					);
				} else {
					log::error!("failed to read tile {coord:?} body from '{url}': {e}");
				}
				return None;
			}
		};

		let blob = Blob::from(bytes.to_vec());
		let tile = Tile::from_blob(blob, TileCompression::Uncompressed, tile_format);
		return Some((coord, tile));
	}

	if let Some(rt) = &runtime {
		rt.record_error(
			&format!("from_tilejson tile {coord:?}"),
			&anyhow!("HTTP fetch from '{url}' failed after {max_retries} retries"),
		);
	} else {
		log::error!("failed to fetch tile {coord:?} from '{url}' after {max_retries} retries");
	}
	None
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("tilejson", &self.url)
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("tile_pyramid not set"))
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		Ok(fetch_tile(
			self.client.clone(),
			self.tile_url_template.clone(),
			*coord,
			self.tile_format,
			self.max_retries,
			Some(self.runtime.clone()),
		)
		.await
		.map(|(_, tile)| tile))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_coord| {
			Some(())
		}))
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_tilejson::tile_stream {bbox:?}");
		let client = self.client.clone();
		let template = self.tile_url_template.clone();
		let tile_format = self.tile_format;
		let max_retries = self.max_retries;
		let max_concurrent = self.max_concurrent_requests;
		let runtime = self.runtime.clone();

		let s = stream::iter(bbox.into_iter_coords())
			.map(move |coord| {
				let client = client.clone();
				let template = template.clone();
				let runtime = runtime.clone();
				tokio::spawn(
					async move { fetch_tile(client, template, coord, tile_format, max_retries, Some(runtime)).await },
				)
			})
			.buffer_unordered(max_concurrent)
			.filter_map({
				let runtime = self.runtime.clone();
				move |result| {
					let runtime = runtime.clone();
					async move {
						match result {
							Ok(item) => item,
							Err(e) => {
								runtime.record_error("from_tilejson task", &anyhow!("tokio join error: {e}"));
								None
							}
						}
					}
				}
			});

		Ok(TileStream { inner: s.boxed() })
	}
}

crate::operations::macros::define_read_factory!("from_tilejson", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	// ── detect_tile_format ──────────────────────────────────────────────

	#[rstest]
	#[case(".pbf", TileFormat::MVT)]
	#[case(".png", TileFormat::PNG)]
	#[case(".webp", TileFormat::WEBP)]
	#[case(".jpg", TileFormat::JPG)]
	#[case(".jpeg", TileFormat::JPG)]
	#[case(".avif", TileFormat::AVIF)]
	#[case(".geojson", TileFormat::GEOJSON)]
	#[case(".json", TileFormat::JSON)]
	#[case(".svg", TileFormat::SVG)]
	#[case(".topojson", TileFormat::TOPOJSON)]
	#[case(".PNG", TileFormat::PNG)]
	#[test]
	fn detect_format_from_url_extension(#[case] ext: &str, #[case] expected: TileFormat) {
		let url = format!("https://example.com/{{z}}/{{x}}/{{y}}{ext}");
		assert_eq!(detect_tile_format(&TileJSON::default(), &url).unwrap(), expected);
	}

	#[rstest]
	#[case("https://example.com/{z}/{x}/{y}")]
	#[case("https://example.com/{z}/{x}/{y}.xyz")]
	#[case("https://example.com/{z}/{x}/{y}?format=png")]
	#[test]
	fn detect_format_from_url_fails(#[case] url: &str) {
		assert!(detect_tile_format(&TileJSON::default(), url).is_err());
	}

	#[test]
	fn detect_format_from_tilejson_field() {
		let tilejson = TileJSON {
			tile_format: Some(TileFormat::MVT),
			..Default::default()
		};
		assert_eq!(
			detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}").unwrap(),
			TileFormat::MVT,
		);
	}

	#[test]
	fn detect_format_tilejson_takes_precedence_over_url() {
		let tilejson = TileJSON {
			tile_format: Some(TileFormat::JPG),
			..Default::default()
		};
		assert_eq!(
			detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}.png").unwrap(),
			TileFormat::JPG,
		);
	}

	// ── extract_tile_url ────────────────────────────────────────────────

	#[rstest]
	#[case(
		r#"{"tilejson":"3.0.0","tiles":["https://example.com/{z}/{x}/{y}.pbf"]}"#,
		"https://example.com/{z}/{x}/{y}.pbf"
	)]
	#[case(
		r#"{"tilejson":"3.0.0","tiles":["https://a.example.com/{z}/{x}/{y}.pbf","https://b.example.com/{z}/{x}/{y}.pbf"]}"#,
		"https://a.example.com/{z}/{x}/{y}.pbf"
	)]
	#[case(
		r#"{"tilejson":"3.0.0","tiles":["https://tiles.example.com/v1/{z}/{x}/{y}.mvt?token=abc123"]}"#,
		"https://tiles.example.com/v1/{z}/{x}/{y}.mvt?token=abc123"
	)]
	#[test]
	fn extract_url_valid(#[case] json: &str, #[case] expected: &str) -> Result<()> {
		let tilejson = TileJSON::try_from(json)?;
		assert_eq!(extract_tile_url(&tilejson)?, expected);
		Ok(())
	}

	#[test]
	fn extract_url_missing_tiles() {
		assert!(extract_tile_url(&TileJSON::default()).is_err());
	}

	#[test]
	fn extract_url_empty_tiles() -> Result<()> {
		let tilejson = TileJSON::try_from(r#"{"tilejson":"3.0.0","tiles":[]}"#)?;
		assert!(extract_tile_url(&tilejson).is_err());
		Ok(())
	}

	// ── build_tile_url ──────────────────────────────────────────────────

	#[rstest]
	#[case("https://example.com/{z}/{x}/{y}.pbf", 3, 5, 7, "https://example.com/3/5/7.pbf")]
	#[case(
		"https://tiles.example.com/data/{z}/{x}/{y}.png",
		0,
		0,
		0,
		"https://tiles.example.com/data/0/0/0.png"
	)]
	#[case(
		"https://example.com/{z}/{x}/{y}.pbf",
		18,
		131072,
		262143,
		"https://example.com/18/131072/262143.pbf"
	)]
	#[case(
		"https://example.com/{z}/{x}/{y}.pbf?token=secret&v=2",
		5,
		10,
		20,
		"https://example.com/5/10/20.pbf?token=secret&v=2"
	)]
	#[case(
		"https://{z}.tiles.example.com/{x}/{y}.png",
		4,
		8,
		12,
		"https://4.tiles.example.com/8/12.png"
	)]
	#[case(
		"https://example.com/{z}/{x}/{y}?zoom={z}",
		7,
		100,
		50,
		"https://example.com/7/100/50?zoom=7"
	)]
	#[test]
	fn build_url(#[case] template: &str, #[case] z: u8, #[case] x: u32, #[case] y: u32, #[case] expected: &str) {
		let coord = TileCoord::new(z, x, y).unwrap();
		assert_eq!(build_tile_url(template, &coord), expected);
	}

	// ── fetch_tile ──────────────────────────────────────────────────────

	// Local mock HTTP server. Spawns an axum app on a free port and returns
	// the base URL (e.g. `http://127.0.0.1:54321`).
	async fn spawn_mock_server<F>(router_builder: F) -> String
	where
		F: FnOnce() -> axum::Router,
	{
		use tokio::net::TcpListener;
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();
		let app = router_builder();
		tokio::spawn(async move {
			axum::serve(listener, app).await.unwrap();
		});
		// Give the runtime a tick to register the spawn.
		tokio::task::yield_now().await;
		format!("http://{addr}")
	}

	#[tokio::test]
	async fn fetch_tile_success_returns_blob() {
		let base = spawn_mock_server(|| {
			// axum rejects multi-param segments like `/{z}/{x}/{y}.png`; use a
			// catch-all fallback that matches every tile path.
			axum::Router::new().fallback(axum::routing::get(|| async { "pixel_bytes".to_string() }))
		})
		.await;

		let template = format!("{base}/{{z}}/{{x}}/{{y}}.png");
		let client = reqwest::Client::new();
		let result = fetch_tile(
			client,
			template,
			TileCoord::new(2, 1, 3).unwrap(),
			TileFormat::PNG,
			0,
			None,
		)
		.await;

		let (coord, tile) = result.expect("expected Some on 200 OK");
		assert_eq!(coord.level, 2);
		assert_eq!(coord.x, 1);
		assert_eq!(coord.y, 3);
		let blob = tile.into_blob(&TileCompression::Uncompressed).unwrap();
		assert_eq!(blob.as_slice(), b"pixel_bytes");
	}

	#[tokio::test]
	async fn fetch_tile_404_returns_none() {
		let base =
			spawn_mock_server(|| axum::Router::new().fallback(|| async { (axum::http::StatusCode::NOT_FOUND, "nope") }))
				.await;

		let result = fetch_tile(
			reqwest::Client::new(),
			format!("{base}/{{z}}/{{x}}/{{y}}.png"),
			TileCoord::new(0, 0, 0).unwrap(),
			TileFormat::PNG,
			0,
			None,
		)
		.await;
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn fetch_tile_http_5xx_records_error_after_retries() {
		use versatiles_container::TilesRuntime;
		let base = spawn_mock_server(|| {
			axum::Router::new().fallback(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") })
		})
		.await;

		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.abort_on_error(true)
			.build();
		let result = fetch_tile(
			reqwest::Client::new(),
			format!("{base}/{{z}}/{{x}}/{{y}}.png"),
			TileCoord::new(1, 0, 0).unwrap(),
			TileFormat::PNG,
			1, // 1 retry → 2 attempts total
			Some(runtime.clone()),
		)
		.await;
		assert!(result.is_none());
		assert_eq!(runtime.error_count(), 1, "exhausted retries should record one error");
		assert!(runtime.had_errors());
	}

	#[tokio::test]
	async fn fetch_tile_http_5xx_without_runtime_returns_none() {
		let base = spawn_mock_server(|| {
			axum::Router::new().fallback(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") })
		})
		.await;

		let result = fetch_tile(
			reqwest::Client::new(),
			format!("{base}/{{z}}/{{x}}/{{y}}.png"),
			TileCoord::new(0, 0, 0).unwrap(),
			TileFormat::PNG,
			0,
			None, // no runtime — just logs
		)
		.await;
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn fetch_tile_connection_refused_returns_none() {
		let client = reqwest::Client::builder()
			.timeout(Duration::from_millis(500))
			.build()
			.unwrap();
		let result = fetch_tile(
			client,
			"http://127.0.0.1:1/{z}/{x}/{y}.pbf".to_string(),
			TileCoord::new(0, 0, 0).unwrap(),
			TileFormat::MVT,
			0,
			None,
		)
		.await;
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn fetch_tile_connection_refused_records_error() {
		use versatiles_container::TilesRuntime;
		let client = reqwest::Client::builder()
			.timeout(Duration::from_millis(500))
			.build()
			.unwrap();
		let runtime = TilesRuntime::builder()
			.silent_progress(true)
			.abort_on_error(true)
			.build();
		let result = fetch_tile(
			client,
			"http://127.0.0.1:1/{z}/{x}/{y}.pbf".to_string(),
			TileCoord::new(0, 0, 0).unwrap(),
			TileFormat::MVT,
			0,
			Some(runtime.clone()),
		)
		.await;
		assert!(result.is_none());
		assert_eq!(runtime.error_count(), 1);
	}

	// ── is_retryable_error ──────────────────────────────────────────────

	#[tokio::test]
	async fn is_retryable_error_flags_timeouts_and_connect() {
		// Build a client that will timeout connecting to an unreachable IP.
		let client = reqwest::Client::builder()
			.timeout(Duration::from_millis(10))
			.build()
			.unwrap();
		let err = client.get("http://10.255.255.1:1/nothing").send().await.unwrap_err();
		// At least one of connect / timeout / body should be true for a timeout
		// on an unroutable address.
		assert!(is_retryable_error(&err));
	}

	// ── Operation::build via VPL ────────────────────────────────────────

	#[tokio::test]
	async fn operation_build_happy_path() -> Result<()> {
		use crate::PipelineFactory;

		// Single server that serves tiles.json at /tiles.json (with an absolute
		// tiles URL template pointing back at itself) and tiles at anything
		// else via a catch-all.
		let base_holder: std::sync::Arc<std::sync::OnceLock<String>> = std::sync::Arc::new(std::sync::OnceLock::new());
		let holder = base_holder.clone();
		let base = spawn_mock_server(move || {
			let holder_for_json = holder.clone();
			axum::Router::new()
				.route(
					"/tiles.json",
					axum::routing::get(move || {
						let holder = holder_for_json.clone();
						async move {
							let base = holder.get().cloned().unwrap_or_default();
							let body = format!(
								r#"{{"tilejson":"3.0.0","tiles":["{base}/{{z}}/{{x}}/{{y}}.png"],"minzoom":0,"maxzoom":2}}"#
							);
							([("content-type", "application/json")], body)
						}
					}),
				)
				.fallback(axum::routing::get(|| async { "pixel_bytes".to_string() }))
		})
		.await;
		base_holder.set(base.clone()).unwrap();

		let factory = PipelineFactory::new_dummy();
		let vpl = crate::vpl::VPLNode::try_from_str(&format!(r#"from_tilejson url="{base}/tiles.json""#))?;
		let source = Operation::build(vpl, &factory).await?;
		assert_eq!(*source.metadata().tile_format(), TileFormat::PNG);
		assert!(source.tilejson().zoom_max().is_some());
		Ok(())
	}

	#[tokio::test]
	async fn operation_build_errors_on_http_5xx() {
		use crate::PipelineFactory;
		let base = spawn_mock_server(|| {
			axum::Router::new().fallback(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") })
		})
		.await;
		let factory = PipelineFactory::new_dummy();
		let vpl = crate::vpl::VPLNode::try_from_str(&format!(r#"from_tilejson url="{base}/tiles.json""#)).unwrap();
		let err = Operation::build(vpl, &factory).await.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("HTTP") || msg.contains("500"));
	}

	// ── tile_stream and tile via a mock server ──────────────────────────

	#[tokio::test]
	async fn tile_and_tile_stream_happy_path() -> Result<()> {
		use crate::PipelineFactory;
		use futures::StreamExt as _;

		// Server: tiles.json at /tiles.json; all other paths return a fixed body.
		let base_holder: std::sync::Arc<std::sync::OnceLock<String>> = std::sync::Arc::new(std::sync::OnceLock::new());
		let holder = base_holder.clone();
		let base = spawn_mock_server(move || {
			let holder_for_json = holder.clone();
			axum::Router::new()
				.route(
					"/tiles.json",
					axum::routing::get(move || {
						let holder = holder_for_json.clone();
						async move {
							let base = holder.get().cloned().unwrap_or_default();
							let body = format!(
								r#"{{"tilejson":"3.0.0","tiles":["{base}/{{z}}/{{x}}/{{y}}.png"],"minzoom":0,"maxzoom":2}}"#
							);
							([("content-type", "application/json")], body)
						}
					}),
				)
				.fallback(axum::routing::get(|| async { "tile_body".to_string() }))
		})
		.await;
		base_holder.set(base.clone()).unwrap();

		let factory = PipelineFactory::new_dummy();
		let vpl = crate::vpl::VPLNode::try_from_str(&format!(
			r#"from_tilejson url="{base}/tiles.json" max_concurrent_requests=2"#
		))?;
		let source = Operation::build(vpl, &factory).await?;

		// Single-tile path.
		let tile = source
			.tile(&TileCoord::new(1, 0, 0).unwrap())
			.await?
			.expect("tile should be present");
		let blob = tile.into_blob(&TileCompression::Uncompressed)?;
		assert_eq!(blob.as_slice(), b"tile_body");

		// Stream path: fetch all tiles at z=1 (2×2 = 4 tiles).
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let stream = source.tile_stream(bbox).await?;
		let collected: Vec<_> = stream.inner.collect().await;
		assert_eq!(collected.len(), 4);
		for (_coord, tile) in collected {
			let b = tile.into_blob(&TileCompression::Uncompressed)?;
			assert_eq!(b.as_slice(), b"tile_body");
		}
		Ok(())
	}
}
