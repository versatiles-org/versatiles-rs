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

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use tokio::time::sleep;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime, Traversal};
use versatiles_core::{
	Blob, GeoBBox, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream,
};
use versatiles_derive::context;

use crate::{PipelineFactory, helpers::location::HttpUrl, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads tiles from a remote tile server described by a TileJSON endpoint.
///
/// The TileJSON document is fetched once when the pipeline is built, and each
/// tile is then requested individually through the URL template in its `tiles`
/// array. A relative template is resolved against the TileJSON's own URL.
///
/// A URL that points at something other than a TileJSON, such as a tile
/// container, is refused after its first bytes, with what it appears to be; a
/// container is read with `from_container`.
struct Args {
	/// URL of the TileJSON endpoint.
	url: HttpUrl,
	/// How often to retry a failed tile request. Defaults to `3`.
	#[vpl(default = "3")]
	max_retries: Option<u16>,
	/// How many tile requests may be in flight, at least 1. Defaults to `4`.
	#[vpl(default = "4")]
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

impl Operation {
	#[context("Failed to build from_tilejson operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		let max_retries = u32::from(args.max_retries.unwrap_or(3));
		let max_concurrent_requests = args.max_concurrent_requests.unwrap_or(4) as usize;
		// With no request allowed in flight, the tile stream would wait forever.
		ensure!(
			max_concurrent_requests >= 1,
			"max_concurrent_requests must be at least 1"
		);

		let client = reqwest::Client::builder()
			.user_agent(versatiles_core::io::user_agent())
			.tcp_keepalive(Duration::from_secs(600))
			.use_rustls_tls()
			.build()?;

		let tilejson = fetch_tilejson(&client, args.url.as_str()).await?;

		// Extract tile URL template, resolved against the TileJSON's own URL
		let tile_url_template = resolve_tile_url(args.url.as_str(), &extract_tile_url(&tilejson)?)?;

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
			url: args.url.to_string(),
			runtime: factory.runtime(),
		}) as Box<dyn TileSource>)
	}
}

/// The most a TileJSON is read up to. One is usually a few kilobytes, and even
/// one listing many vector layers stays far below this.
const MAX_TILEJSON_BYTES: usize = 16 * 1024 * 1024;

/// How much of a body that is not JSON is read to say what it is instead: a
/// tar file's signature ends at byte 262.
const SNIFF_BYTES: usize = 264;

/// Fetches and parses the TileJSON at `url`, failing as soon as the body shows
/// it is not one.
///
/// A URL pointing at a tile container instead of its TileJSON is an easy
/// mistake, and the container can be gigabytes. Reading the whole body before
/// parsing it would download all of that only to report a syntax error, so
/// the body is read chunk by chunk: anything that does not start with `{`, the
/// start of every TileJSON, is refused after its first bytes, and so is
/// anything that announces more than [`MAX_TILEJSON_BYTES`]; a body that
/// announces nothing is refused once it gets there.
async fn fetch_tilejson(client: &reqwest::Client, url: &str) -> Result<TileJSON> {
	let mut response = client.get(url).send().await?;
	if !response.status().is_success() {
		bail!("Failed to fetch TileJSON from '{url}': HTTP {}", response.status());
	}
	// Refused once its first bytes are in, not before: they can say what it
	// is, which is more use than only that it is too long.
	let too_long = response
		.content_length()
		.filter(|&length| usize::try_from(length).map_or(true, |length| length > MAX_TILEJSON_BYTES));

	let mut body: Vec<u8> = Vec::new();
	let mut starts_like_json = false;
	while let Some(chunk) = response.chunk().await? {
		body.extend_from_slice(&chunk);
		ensure!(
			body.len() <= MAX_TILEJSON_BYTES,
			"'{url}' is more than {MAX_TILEJSON_BYTES} bytes long, more than a TileJSON can be"
		);
		if starts_like_json {
			continue;
		}
		let text = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&body);
		match text.iter().find(|byte| !byte.is_ascii_whitespace()) {
			Some(b'{') => {
				if let Some(length) = too_long {
					bail!(
						"'{url}' is {length} bytes long, more than a TileJSON can be ({MAX_TILEJSON_BYTES} bytes at most)"
					);
				}
				starts_like_json = true;
			}
			// Enough of it to tell what it is, then no more.
			Some(_) if body.len() >= SNIFF_BYTES => break,
			_ => {}
		}
	}
	if !starts_like_json {
		bail!("'{url}' is not a TileJSON: {}", describe_non_json(url, &body));
	}

	let text = String::from_utf8(body).map_err(|_| anyhow!("'{url}' is not a TileJSON: it is not UTF-8 text"))?;
	// The byte order mark `response.text()` used to drop.
	let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
	TileJSON::try_from(text).with_context(|| format!("parsing the TileJSON from '{url}'"))
}

/// What a body that is not a TileJSON appears to be, by the signature it
/// starts with, and what to do with it.
fn describe_non_json(url: &str, body: &[u8]) -> String {
	let container = if body.starts_with(b"versatiles_v") {
		Some("a VersaTiles container")
	} else if body.starts_with(b"PMTiles") {
		Some("a PMTiles container")
	} else if body.starts_with(b"SQLite format 3\0") {
		Some("an SQLite database, as an MBTiles container is")
	} else if body.get(257..262) == Some(b"ustar") {
		Some("a tar archive")
	} else {
		None
	};
	if let Some(container) = container {
		return format!("it is {container}, which `from_container filename=\"{url}\"` reads");
	}
	let kind = if body.is_empty() {
		"it is empty"
	} else if body.starts_with(&[0x1f, 0x8b]) {
		"it is gzip-compressed data"
	} else if body.starts_with(b"\x89PNG") {
		"it is a PNG image"
	} else if body.starts_with(&[0xff, 0xd8, 0xff]) {
		"it is a JPEG image"
	} else if body.starts_with(b"RIFF") && body.get(8..12) == Some(b"WEBP") {
		"it is a WebP image"
	} else if body.starts_with(b"PK\x03\x04") {
		"it is a ZIP archive"
	} else if body.trim_ascii_start().starts_with(b"<") {
		"it looks like an HTML or XML page"
	} else {
		"it does not start with `{`"
	};
	kind.to_string()
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

/// Resolves a `tiles` template against the URL of the TileJSON document, as
/// the VersaTiles extension of TileJSON allows relative templates.
///
/// Joining percent-encodes the braces, so `%7B`/`%7D` are turned back into
/// `{`/`}` afterwards. An absolute template is returned as written: it needs
/// no resolving, and a placeholder in its host would not survive parsing.
fn resolve_tile_url(tilejson_url: &str, template: &str) -> Result<String> {
	if has_scheme(template) {
		return Ok(template.to_string());
	}
	let base = reqwest::Url::parse(tilejson_url)
		.map_err(|e| anyhow!("TileJSON URL '{tilejson_url}' is not a valid base URL: {e}"))?;
	let resolved = base
		.join(template)
		.map_err(|e| anyhow!("cannot resolve tile URL '{template}' against '{tilejson_url}': {e}"))?;
	Ok(resolved
		.as_str()
		.replace("%7B", "{")
		.replace("%7b", "{")
		.replace("%7D", "}")
		.replace("%7d", "}"))
}

/// Whether `text` starts with a URL scheme (`https:`, `http:`, …).
fn has_scheme(text: &str) -> bool {
	text.split_once(':').is_some_and(|(scheme, _)| {
		let mut chars = scheme.chars();
		chars.next().is_some_and(|c| c.is_ascii_alphabetic())
			&& chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
	})
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

/// Exponential backoff unit for retry waits (seconds in prod, ms in tests).
#[cfg(not(test))]
const BACKOFF: fn(u32) -> Duration = |exp| Duration::from_secs(1 << exp);
#[cfg(test)]
const BACKOFF: fn(u32) -> Duration = |exp| Duration::from_millis(1 << exp);

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
			let backoff = BACKOFF(attempt - 1);
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
		// Clipped as the coordinate stream is: every coordinate here is an HTTP
		// request, and inside `from_stacked` the bbox asked for is the union of
		// all sources — one request per tile this server never had.
		let bbox = self.metadata.intersection_bbox(&bbox);
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
	use rstest::rstest;

	use super::*;

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
	#[case(".mlt", TileFormat::MLT)]
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

	// ── resolve_tile_url ────────────────────────────────────────────────

	#[rstest]
	#[case::spec_example(
		"https://example.org/tiles/osm/tiles.json",
		"{z}/{x}/{y}",
		"https://example.org/tiles/osm/{z}/{x}/{y}"
	)]
	#[case::with_extension(
		"https://example.org/tiles/osm/tiles.json",
		"{z}/{x}/{y}.pbf",
		"https://example.org/tiles/osm/{z}/{x}/{y}.pbf"
	)]
	#[case::root_relative(
		"https://example.org/tiles/osm/tiles.json",
		"/other/{z}/{x}/{y}.png",
		"https://example.org/other/{z}/{x}/{y}.png"
	)]
	#[case::parent(
		"https://example.org/tiles/osm/tiles.json",
		"../{z}/{x}/{y}.png",
		"https://example.org/tiles/{z}/{x}/{y}.png"
	)]
	#[case::protocol_relative(
		"https://example.org/tiles.json",
		"//cdn.example.org/{z}/{x}/{y}.png",
		"https://cdn.example.org/{z}/{x}/{y}.png"
	)]
	#[case::query(
		"https://example.org/tiles/tiles.json?key=abc",
		"{z}/{x}/{y}.png?zoom={z}",
		"https://example.org/tiles/{z}/{x}/{y}.png?zoom={z}"
	)]
	#[case::already_encoded_lowercase(
		"https://example.org/tiles/tiles.json",
		"%7bz%7d/%7bx%7d/%7by%7d.png",
		"https://example.org/tiles/{z}/{x}/{y}.png"
	)]
	#[case::absolute(
		"https://example.org/tiles.json",
		"https://tiles.example.com/{z}/{x}/{y}.pbf",
		"https://tiles.example.com/{z}/{x}/{y}.pbf"
	)]
	#[case::absolute_placeholder_in_host(
		"https://example.org/tiles.json",
		"https://{z}.tiles.example.com/{x}/{y}.png",
		"https://{z}.tiles.example.com/{x}/{y}.png"
	)]
	#[test]
	fn resolve_url(#[case] base: &str, #[case] template: &str, #[case] expected: &str) {
		assert_eq!(resolve_tile_url(base, template).unwrap(), expected);
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
	async fn operation_build_resolves_relative_tile_url() -> Result<()> {
		use crate::PipelineFactory;

		// The template is relative to the TileJSON, so tiles live under /data/.
		let base = spawn_mock_server(|| {
			axum::Router::new()
				.route(
					"/data/tiles.json",
					axum::routing::get(|| async {
						r#"{"tilejson":"3.0.0","tiles":["{z}/{x}/{y}.png"],"minzoom":0,"maxzoom":2}"#
					}),
				)
				.route("/data/1/0/1.png", axum::routing::get(|| async { "relative_tile" }))
				.fallback(|| async { axum::http::StatusCode::NOT_FOUND })
		})
		.await;

		let factory = PipelineFactory::new_dummy();
		let vpl = crate::vpl::VPLNode::try_from_str(&format!(r#"from_tilejson url="{base}/data/tiles.json""#))?;
		let source = Operation::build(vpl, &factory).await?;
		assert_eq!(*source.metadata().tile_format(), TileFormat::PNG);

		let tile = source
			.tile(&TileCoord::new(1, 0, 1).unwrap())
			.await?
			.expect("tile should be present");
		assert_eq!(
			tile.into_blob(&TileCompression::Uncompressed)?.as_slice(),
			b"relative_tile"
		);
		Ok(())
	}

	/// `buffer_unordered(0)` never yields, so zero requests in flight hung the
	/// tile stream. It is refused before anything is fetched.
	#[tokio::test]
	async fn operation_build_refuses_zero_concurrent_requests() {
		let factory = crate::PipelineFactory::new_dummy();
		let vpl = crate::vpl::VPLNode::try_from_str(
			r#"from_tilejson url="http://127.0.0.1:1/tiles.json" max_concurrent_requests=0"#,
		)
		.unwrap();
		let err = Operation::build(vpl, &factory).await.unwrap_err();
		assert!(
			format!("{err:#}").contains("max_concurrent_requests must be at least 1"),
			"{err:#}"
		);
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
		use futures::StreamExt as _;

		use crate::PipelineFactory;

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

	/// The tile stream used not to clip to the pyramid: every coordinate asked
	/// for became a request, also those outside the TileJSON's bounds.
	#[tokio::test]
	async fn tile_stream_requests_only_tiles_inside_the_bounds() -> Result<()> {
		use std::sync::atomic::{AtomicUsize, Ordering};

		use crate::PipelineFactory;

		let requests = std::sync::Arc::new(AtomicUsize::new(0));
		let counter = requests.clone();
		let base_holder: std::sync::Arc<std::sync::OnceLock<String>> = std::sync::Arc::new(std::sync::OnceLock::new());
		let holder = base_holder.clone();
		let base = spawn_mock_server(move || {
			let holder = holder.clone();
			let counter = counter.clone();
			axum::Router::new()
				.route(
					"/tiles.json",
					axum::routing::get(move || {
						let base = holder.get().cloned().unwrap_or_default();
						async move {
							format!(
								r#"{{"tilejson":"3.0.0","tiles":["{base}/{{z}}/{{x}}/{{y}}.png"],"minzoom":0,"maxzoom":3,"bounds":[10,42,20,50]}}"#
							)
						}
					}),
				)
				.fallback(axum::routing::get(move || {
					counter.fetch_add(1, Ordering::SeqCst);
					async { "tile_body".to_string() }
				}))
		})
		.await;
		base_holder.set(base.clone()).unwrap();

		let vpl = crate::vpl::VPLNode::try_from_str(&format!(r#"from_tilejson url="{base}/tiles.json""#))?;
		let source = Operation::build(vpl, &PipelineFactory::new_dummy()).await?;
		let tiles = source.tile_stream(TileBBox::new_full(3)?).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);
		assert_eq!(
			requests.load(Ordering::SeqCst),
			1,
			"only the tile inside the bounds is requested"
		);
		Ok(())
	}

	// ── fetch_tilejson: failing fast on what is not a TileJSON ──────────

	/// A body that never ends, `head` then `filler` over and over, sent as it
	/// is made, the way a large container download arrives. `length` is
	/// announced as its `Content-Length`, if given.
	async fn endless(head: &'static [u8], filler: &'static [u8], length: Option<u64>) -> String {
		spawn_mock_server(move || {
			axum::Router::new().fallback(axum::routing::get(move || async move {
				let chunks = futures::stream::once(async move { Ok::<_, std::convert::Infallible>(head.to_vec()) })
					.chain(futures::stream::repeat_with(move || Ok(filler.to_vec())));
				let mut response = axum::response::Response::new(axum::body::Body::from_stream(chunks));
				if let Some(length) = length {
					response
						.headers_mut()
						.insert(axum::http::header::CONTENT_LENGTH, length.into());
				}
				response
			}))
		})
		.await
	}

	async fn fetch_error(url: &str) -> String {
		let fetched = tokio::time::timeout(Duration::from_secs(10), fetch_tilejson(&reqwest::Client::new(), url))
			.await
			.expect("refused in time, not read to the end");
		format!("{:#}", fetched.unwrap_err())
	}

	/// The mistake this exists for: a container's URL instead of its
	/// TileJSON's. Refused after its first bytes, of a body that never ends,
	/// and the message says what to use instead, also when the server
	/// announces the container's size, as a file server does.
	#[rstest]
	#[case::unannounced(None)]
	#[case::announced(Some(66_636_533_284))]
	#[tokio::test]
	async fn a_container_is_refused_after_its_first_bytes(#[case] length: Option<u64>) {
		let url = format!(
			"{}/world.versatiles",
			endless(b"versatiles_v02", &[0; 16384], length).await
		);
		let error = fetch_error(&url).await;
		assert!(error.contains("a VersaTiles container"), "{error}");
		assert!(error.contains(&format!("from_container filename=\"{url}\"")), "{error}");
	}

	/// A body that starts like JSON but goes on and on is refused once it
	/// passes the limit.
	#[tokio::test]
	async fn a_body_past_the_limit_is_refused() {
		let url = format!("{}/tiles.json", endless(b"{\"a\":\"", &[b'x'; 16384], None).await);
		let error = fetch_error(&url).await;
		assert!(error.contains("more than"), "{error}");
	}

	/// A `Content-Length` past the limit is refused before the body is read.
	#[tokio::test]
	async fn an_announced_length_past_the_limit_is_refused_unread() {
		let url = format!("{}/tiles.json", endless(b"{", b" ", Some(1 << 40)).await);
		let error = fetch_error(&url).await;
		assert!(error.contains("1099511627776 bytes long"), "{error}");
	}

	/// Leading whitespace and a byte order mark are still a TileJSON.
	#[tokio::test]
	async fn a_tilejson_with_a_bom_and_whitespace_is_read() -> Result<()> {
		let base = spawn_mock_server(|| {
			axum::Router::new().fallback(axum::routing::get(|| async {
				"\u{feff}\n  {\"tilejson\":\"3.0.0\",\"tiles\":[\"{z}/{x}/{y}.png\"]}"
			}))
		})
		.await;
		let tilejson = fetch_tilejson(&reqwest::Client::new(), &format!("{base}/tiles.json")).await?;
		assert_eq!(extract_tile_url(&tilejson)?, "{z}/{x}/{y}.png");
		Ok(())
	}

	#[rstest]
	#[case::pmtiles(b"PMTiles\x03".to_vec(), "a PMTiles container")]
	#[case::mbtiles(b"SQLite format 3\0".to_vec(), "MBTiles")]
	#[case::tar([vec![b'a'; 257], b"ustar\0".to_vec()].concat(), "a tar archive")]
	#[case::gzip(vec![0x1f, 0x8b, 8, 0], "gzip")]
	#[case::png(b"\x89PNG\r\n".to_vec(), "PNG image")]
	#[case::webp(b"RIFF\0\0\0\0WEBPVP8 ".to_vec(), "WebP image")]
	#[case::html(b"\n<!DOCTYPE html>".to_vec(), "HTML")]
	#[case::empty(Vec::new(), "empty")]
	#[case::other(b"hello".to_vec(), "does not start with `{`")]
	#[test]
	fn what_is_not_json_is_named(#[case] body: Vec<u8>, #[case] expected: &str) {
		let description = describe_non_json("https://example.org/x", &body);
		assert!(description.contains(expected), "{description}");
	}
}
