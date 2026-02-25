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
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{
	Blob, GeoBBox, TileBBox, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, TileStream,
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

#[derive(Debug)]
struct Operation {
	client: reqwest::Client,
	tile_url_template: String,
	tile_format: TileFormat,
	max_retries: u32,
	max_concurrent_requests: usize,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	url: String,
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_tilejson operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, _factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
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

		// Build bbox pyramid from TileJSON bounds/zoom
		let min_zoom = tilejson.min_zoom().unwrap_or(0);
		let max_zoom = tilejson.max_zoom().unwrap_or(22);
		let geo_bbox = tilejson
			.bounds
			.unwrap_or_else(|| GeoBBox::new(-180.0, -85.05112878, 180.0, 85.05112878).unwrap());
		let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(min_zoom, max_zoom, &geo_bbox);

		let metadata = TileSourceMetadata::new(
			tile_format,
			TileCompression::Uncompressed,
			bbox_pyramid,
			Traversal::new_any(),
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
		}) as Box<dyn TileSource>)
	}
}

fn extract_tile_url(tilejson: &TileJSON) -> Result<String> {
	let obj = tilejson.as_object();
	let tiles_value = obj
		.get("tiles")
		.ok_or_else(|| anyhow!("TileJSON is missing 'tiles' array"))?;
	let tiles_array = tiles_value.as_array()?;
	let tiles = tiles_array.as_string_vec()?;
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
				log::error!("failed to fetch tile {coord:?} from '{url}': {e}");
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
			log::error!(
				"failed to fetch tile {coord:?} from '{url}': HTTP {}",
				response.status()
			);
			return None;
		}

		let bytes = match response.bytes().await {
			Ok(b) => b,
			Err(e) if is_retryable_error(&e) && attempt < max_retries => {
				log::warn!("retryable error reading tile {coord:?} body: {e}");
				continue;
			}
			Err(e) => {
				log::error!("failed to read tile {coord:?} body from '{url}': {e}");
				return None;
			}
		};

		let blob = Blob::from(bytes.to_vec());
		let tile = Tile::from_blob(blob, TileCompression::Uncompressed, tile_format);
		return Some((coord, tile));
	}

	log::error!("failed to fetch tile {coord:?} from '{url}' after {max_retries} retries");
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

	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		Ok(fetch_tile(
			self.client.clone(),
			self.tile_url_template.clone(),
			*coord,
			self.tile_format,
			self.max_retries,
		)
		.await
		.map(|(_, tile)| tile))
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::debug!("get_tile_stream {bbox:?}");
		let client = self.client.clone();
		let template = self.tile_url_template.clone();
		let tile_format = self.tile_format;
		let max_retries = self.max_retries;
		let max_concurrent = self.max_concurrent_requests;

		let s = stream::iter(bbox.into_iter_coords())
			.map(move |coord| {
				let client = client.clone();
				let template = template.clone();
				tokio::spawn(async move { fetch_tile(client, template, coord, tile_format, max_retries).await })
			})
			.buffer_unordered(max_concurrent)
			.filter_map(|result| async {
				match result {
					Ok(item) => item,
					Err(e) => {
						log::error!("Task join error: {e:?}");
						None
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

	#[test]
	fn test_detect_tile_format_from_tilejson_field() {
		let tilejson = TileJSON {
			tile_format: Some(TileFormat::MVT),
			..Default::default()
		};
		let result = detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}");
		assert_eq!(result.unwrap(), TileFormat::MVT);
	}

	#[test]
	fn test_detect_tile_format_from_url_pbf() {
		let tilejson = TileJSON::default();
		let result = detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}.pbf");
		assert_eq!(result.unwrap(), TileFormat::MVT);
	}

	#[test]
	fn test_detect_tile_format_from_url_png() {
		let tilejson = TileJSON::default();
		let result = detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}.png");
		assert_eq!(result.unwrap(), TileFormat::PNG);
	}

	#[test]
	fn test_detect_tile_format_missing() {
		let tilejson = TileJSON::default();
		let result = detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}");
		assert!(result.is_err());
	}

	#[test]
	fn test_detect_tile_format_tilejson_takes_precedence() {
		let tilejson = TileJSON {
			tile_format: Some(TileFormat::JPG),
			..Default::default()
		};
		let result = detect_tile_format(&tilejson, "https://example.com/{z}/{x}/{y}.png");
		assert_eq!(result.unwrap(), TileFormat::JPG);
	}

	#[test]
	fn test_extract_tile_url_valid() -> Result<()> {
		let tilejson = TileJSON::try_from(r#"{"tilejson":"3.0.0","tiles":["https://example.com/{z}/{x}/{y}.pbf"]}"#)?;
		let url = extract_tile_url(&tilejson)?;
		assert_eq!(url, "https://example.com/{z}/{x}/{y}.pbf");
		Ok(())
	}

	#[test]
	fn test_extract_tile_url_missing_tiles() {
		let tilejson = TileJSON::default();
		let result = extract_tile_url(&tilejson);
		assert!(result.is_err());
	}

	#[test]
	fn test_extract_tile_url_empty_tiles() -> Result<()> {
		let tilejson = TileJSON::try_from(r#"{"tilejson":"3.0.0","tiles":[]}"#)?;
		let result = extract_tile_url(&tilejson);
		assert!(result.is_err());
		Ok(())
	}

	#[test]
	fn test_build_tile_url() {
		let template = "https://example.com/{z}/{x}/{y}.pbf";
		let coord = TileCoord::new(3, 5, 7).unwrap();
		let url = build_tile_url(template, &coord);
		assert_eq!(url, "https://example.com/3/5/7.pbf");
	}

	#[test]
	fn test_build_tile_url_origin() {
		let template = "https://tiles.example.com/data/{z}/{x}/{y}.png";
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let url = build_tile_url(template, &coord);
		assert_eq!(url, "https://tiles.example.com/data/0/0/0.png");
	}

	#[test]
	fn test_build_tile_url_high_zoom() {
		let template = "https://example.com/{z}/{x}/{y}.pbf";
		let coord = TileCoord::new(18, 131072, 262143).unwrap();
		let url = build_tile_url(template, &coord);
		assert_eq!(url, "https://example.com/18/131072/262143.pbf");
	}
}
