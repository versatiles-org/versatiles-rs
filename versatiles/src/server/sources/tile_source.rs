use std::fmt::Debug;

use anyhow::Result;
use versatiles_container::{SharedTileSource, TileSource};
use versatiles_core::{Blob, TileCompression, TileCoord, TileType};
use versatiles_derive::context;

use super::{super::utils::Url, SourceResponse, TileResponse};

// TileSource struct definition
#[derive(Clone)]
pub struct ServerTileSource {
	pub prefix: Url,
	pub id: String,
	reader: SharedTileSource, // NO MORE MUTEX! 🚀
	pub tile_mime: String,
	pub compression: TileCompression,
	tile_type: TileType,
}

impl ServerTileSource {
	// Constructor function for creating a TileSource instance
	#[context("creating tile source: id='{id}'")]
	pub fn from(reader: SharedTileSource, id: &str) -> Result<ServerTileSource> {
		let metadata = reader.metadata();
		let tile_mime = metadata.tile_format().as_mime_str().to_string();
		let tile_type = metadata.tile_format().to_type();
		let compression = *metadata.tile_compression();

		Ok(ServerTileSource {
			prefix: Url::new(format!("/tiles/{id}/")).to_dir(),
			id: id.to_owned(),
			reader,
			tile_mime,
			compression,
			tile_type,
		})
	}

	/// A human-readable description of where these tiles come from, e.g.
	/// `container 'mbtiles' ('/home/me/berlin.mbtiles')`.
	///
	/// This is [`SourceType`](versatiles_container::SourceType)'s `Display`
	/// form: a diagnostic sentence including the full path, suited to a log
	/// line rather than a label. For a short name — `"mbtiles"` — use
	/// `source_type().name()` instead.
	pub fn source_description(&self) -> String {
		self.reader.source_type().to_string()
	}

	// Retrieve the tile data as an HTTP response.
	//
	// Tiles come back in this source's own compression; the caller negotiates
	// against the client's `Accept-Encoding` afterwards, in `handlers::ok_data`.
	#[context("getting tile data: url={url}")]
	pub async fn get_data(&self, url: &Url) -> Result<TileResponse> {
		let parts: Vec<String> = url.as_vec();

		if parts.len() >= 3 {
			// Coordinates that cannot exist are a request for a tile this source
			// does not have, not a fault on our side (#280). Answered as "not
			// found", like a zoom level above the source's maximum, and at debug
			// level: a 500 with a WARN line per request let any client fill the
			// log by asking for zoom 99.
			let coord = match parse_tile_coord(&parts) {
				Ok(coord) => coord,
				Err(error) => {
					log::debug!("no tile '{url}' in '{}': {error:#}", self.id);
					return Ok(TileResponse::NotFound);
				}
			};

			log::debug!("get tile, prefix: {}, coord: {}", self.prefix, coord.to_json());

			// Get tile data
			let tile = self.reader.tile(&coord).await;

			// A tile that cannot be produced is answered as "not found", because
			// that is what a client can do something about. The reason is not
			// the client's business but it is very much the operator's, and
			// discarding it left a 404 as the only evidence that anything went
			// wrong — indistinguishable from a hole in the data.
			//
			// Not as an empty tile, though (#281): that answer is cacheable for
			// as long as a tile is, and a source that hiccuped once would stay
			// blank in every cache in front of it.
			if let Err(error) = &tile {
				log::warn!("could not produce tile {} of '{}': {error:#}", coord.to_json(), self.id);
				return Ok(TileResponse::NotFound);
			}

			return Ok(match tile? {
				Some(tile) => TileResponse::Data(SourceResponse {
					blob: tile.into_blob(&self.compression)?,
					compression: self.compression,
					mime: self.tile_mime.clone(),
				}),
				None if self.is_empty_tile(coord.level) => TileResponse::Empty,
				None => TileResponse::NotFound,
			});
		} else if parts
			.first()
			.is_some_and(|name| name == "meta.json" || name == "tiles.json")
		{
			// Get metadata
			let tile_json = self.build_tile_json().await?;

			return Ok(TileResponse::Data(SourceResponse {
				blob: tile_json,
				compression: TileCompression::Uncompressed,
				mime: "application/json".to_owned(),
			}));
		}

		// If the request is unknown, return a not found response
		Ok(TileResponse::NotFound)
	}

	/// Whether a tile without data at `level` is an empty tile, to be answered
	/// with `204`, rather than a missing one, answered with `404` (#281).
	///
	/// Only a vector tile, and only inside the zoom range. The type decides
	/// because the clients treat the two differently:
	///
	/// - A vector tile looks the same to a map either way, so the 204 that
	///   most tile servers send costs nothing and keeps the browser console
	///   free of an error line per tile.
	/// - A raster tile does not. MapLibre GL JS draws a 204 as a transparent
	///   tile and replaces a 404 by the tile from a lower zoom level. A raster
	///   source with gaps at its high zoom levels — worldwide imagery to zoom
	///   10, aerial imagery to zoom 18 for some countries — depends on that
	///   fallback, and would turn transparent over the rest of the world.
	fn is_empty_tile(&self, level: u8) -> bool {
		self.tile_type == TileType::Vector && self.covers_level(level)
	}

	/// Whether `level` is inside the zoom range this source announces.
	///
	/// The same range `build_tile_json` puts into the TileJSON: the tile
	/// pyramid's if one is known, the source's own `minzoom`/`maxzoom`
	/// otherwise. An end that nobody declared does not limit anything.
	fn covers_level(&self, level: u8) -> bool {
		let pyramid = self.reader.metadata().tile_pyramid();
		let tilejson = self.reader.tilejson();
		let level_min = pyramid
			.as_ref()
			.and_then(|p| p.level_min())
			.or_else(|| tilejson.zoom_min());
		let level_max = pyramid
			.as_ref()
			.and_then(|p| p.level_max())
			.or_else(|| tilejson.zoom_max());
		level_min.is_none_or(|min| level >= min) && level_max.is_none_or(|max| level <= max)
	}

	#[context("building tilejson for tile source id='{}'", self.id)]
	async fn build_tile_json(&self) -> Result<Blob> {
		// Direct access - no lock!
		let mut tilejson = self.reader.tilejson().clone();
		self.reader.metadata().update_tilejson(&mut tilejson);

		let tiles_url = self.prefix.join_as_string("{z}/{x}/{y}");
		tilejson.set_list("tiles", vec![tiles_url])?;

		Ok(tilejson.into())
	}
}

/// Reads the tile coordinate from the first three segments of a path, `z/x/y`.
///
/// `y` may carry an extension (`12/2200/1343.pbf`), which is ignored.
fn parse_tile_coord(parts: &[String]) -> Result<TileCoord> {
	use anyhow::Context as _;

	let level = parts[0].parse::<u8>().context("value for z is not a number")?;
	let x = parts[1].parse::<u32>().context("value for x is not a number")?;

	let y: String = parts[2].chars().take_while(|c| c.is_numeric()).collect();
	let y = y.parse::<u32>().context("value for y is not a number")?;

	TileCoord::new(level, x, y)
}

// Debug implementation for ServerTileSource
impl Debug for ServerTileSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ServerTileSource")
			.field("reader", &self.reader)
			.field("tile_mime", &self.tile_mime)
			.field("compression", &self.compression)
			.finish()
	}
}

#[cfg(test)]
#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "test data is built from literal values"
)]
mod tests {
	use anyhow::Result;
	use rstest::rstest;
	use versatiles_container::{MockReader, MockReaderProfile};
	use versatiles_core::TileJSON;

	use super::*;
	use crate::runtime::create_test_runtime;

	// Test the constructor function for TileSource
	#[tokio::test]
	async fn tile_container_from() -> Result<()> {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?.into_shared();
		let container = ServerTileSource::from(reader, "prefix")?;

		assert_eq!(container.prefix.str, "/tiles/prefix/");
		assert_eq!(
			container.build_tile_json().await?.as_str(),
			"{\"bounds\":[-180,-85.051129,180,85.051129],\"maxzoom\":6,\"minzoom\":2,\"tile_format\":\"image/png\",\"tile_schema\":\"rgb\",\"tile_type\":\"raster\",\"tilejson\":\"3.0.0\",\"tiles\":[\"/tiles/prefix/{z}/{x}/{y}\"],\"type\":\"dummy\"}"
		);

		Ok(())
	}

	// Test the debug function
	#[test]
	fn debug() -> Result<()> {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?.into_shared();
		let container = ServerTileSource::from(reader, "prefix")?;
		// Updated expected output - no more "Mutex { data: ... }"
		let debug_str = format!("{container:?}");
		assert!(
			debug_str.contains("ServerTileSource { reader: MockReader { parameters: TileSourceMetadata {"),
			"unexpected debug output: {debug_str}"
		);
		assert!(debug_str.contains("tile_compression: Uncompressed"));
		assert!(debug_str.contains("tile_format: PNG"));
		assert!(debug_str.contains("tile_mime: \"image/png\""));
		Ok(())
	}

	// Test the get_data method of the ServerTileSource.
	// All three fixtures cover the same central-Berlin bbox at z0..z14
	// (regenerated from osm.versatiles), so they share the same expected
	// bounds and tile coord.
	#[rstest]
	#[case(
		"../testdata/berlin.mbtiles",
		"12/2200/1343",
		("application/vnd.mapbox-vector-tile", "[13.3,52.45,13.46,52.55]", [31, 139, 8, 0], 0, 14)
	)]
	#[case(
		"../testdata/berlin.pmtiles",
		"12/2200/1343",
		("application/vnd.mapbox-vector-tile", "[13.3,52.45,13.46,52.55]", [31, 139, 8, 0], 0, 14)
	)]
	#[case(
		"../testdata/berlin.vpl",
		"12/2200/1343",
		("application/vnd.mapbox-vector-tile", "[13.3,52.45,13.46,52.55]", [31, 139, 8, 0], 0, 14)
	)]
	#[tokio::test]
	async fn tile_container_get_data(
		#[case] filename: &str,
		#[case] coord: &str,
		#[case] expected_tile_json: (&str, &str, [u8; 4], u8, u8),
	) -> Result<()> {
		async fn get_response(container: &mut ServerTileSource, url: &str) -> Result<TileResponse> {
			container.get_data(&Url::from(url)).await
		}

		async fn check_response(container: &mut ServerTileSource, url: &str, mime_type: &str) -> Result<Vec<u8>> {
			let TileResponse::Data(response) = get_response(container, url).await? else {
				panic!("expected data for {url}");
			};
			assert_eq!(response.mime, mime_type);
			Ok(response.blob.into_vec())
		}

		async fn check_status(container: &mut ServerTileSource, url: &str) -> u16 {
			match get_response(container, url).await {
				Ok(TileResponse::Data(_)) => 200,
				Ok(TileResponse::Empty) => 204,
				Ok(TileResponse::NotFound) => 404,
				Err(_) => 500,
			}
		}

		let (exp_mime, exp_bounds, exp_header, exp_minzoom, exp_maxzoom) = expected_tile_json;

		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str(filename).await?;
		let c = &mut ServerTileSource::from(reader, "prefix")?;

		assert_eq!(&check_response(c, coord, exp_mime).await?[0..4], exp_header);

		let tile_json = check_response(c, "meta.json", "application/json").await?;
		let tile_json = TileJSON::try_from(tile_json)?.as_object();
		assert_eq!(tile_json.string("tile_format")?.unwrap(), exp_mime);
		assert_eq!(tile_json.array("bounds")?.unwrap().stringify(), exp_bounds);
		{
			assert_eq!(tile_json.number("minzoom")?.unwrap() as u8, exp_minzoom);
			assert_eq!(tile_json.number("maxzoom")?.unwrap() as u8, exp_maxzoom);
		}

		// A tile that cannot exist is "not found", whatever the reason (#280).
		for url in [
			"x/0/0.png",
			"-1/0/0.png",
			"0/0/-1.png",
			"a/b/c",
			"99/0/0",
			"3/99/0",
			"3/0/8.png",
			"16/0/0.png",
			"15/17600/10745",
		] {
			assert_eq!(check_status(c, url).await, 404, "{url}");
		}

		// Inside the zoom range but outside Berlin: an empty tile (#281).
		for url in ["12/0/0", "14/0/0.pbf", "1/1/1", "12/2200/1300"] {
			assert_eq!(check_status(c, url).await, 204, "{url}");
		}

		Ok(())
	}
}
