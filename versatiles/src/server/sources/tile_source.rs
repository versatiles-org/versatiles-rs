use super::{super::utils::Url, SourceResponse};
use anyhow::Result;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_container::TilesReaderTrait;
use versatiles_core::{Blob, TileCompression, TileCoord, utils::TargetCompression};
use versatiles_derive::context;

// TileSource struct definition
#[derive(Clone)]
pub struct TileSource {
	pub prefix: Url,
	pub id: String,
	reader: Arc<Mutex<Box<dyn TilesReaderTrait>>>,
	pub tile_mime: String,
	pub compression: TileCompression,
}

impl TileSource {
	// Constructor function for creating a TileSource instance
	#[context("creating tile source: id='{id}'")]
	pub fn from(reader: Box<dyn TilesReaderTrait>, id: &str) -> Result<TileSource> {
		let parameters = reader.parameters();
		let tile_mime = parameters.tile_format.as_mime_str().to_string();
		let compression = parameters.tile_compression;

		Ok(TileSource {
			prefix: Url::new(format!("/tiles/{id}/")).to_dir(),
			id: id.to_owned(),
			reader: Arc::new(Mutex::new(reader)),
			tile_mime,
			compression,
		})
	}

	pub async fn get_source_name(&self) -> String {
		let reader = self.reader.lock().await;
		reader.source_name().to_owned()
	}

	// Retrieve the tile data as an HTTP response
	#[context("getting tile data: url={url}")]
	pub async fn get_data(&self, url: &Url, _accept: &TargetCompression) -> Result<Option<SourceResponse>> {
		let parts: Vec<String> = url.as_vec();

		if parts.len() >= 3 {
			// Parse the tile coordinates
			let level = parts[0].parse::<u8>().context("value for z is not a number")?;
			let x = parts[1].parse::<u32>().context("value for x is not a number")?;

			let y: String = parts[2].chars().take_while(|c| c.is_numeric()).collect();
			let y = y.parse::<u32>().context("value for y is not a number")?;

			// Create a TileCoord instance
			let coord = TileCoord::new(level, x, y)?;

			log::debug!("get tile, prefix: {}, coord: {}", self.prefix, coord.as_json());

			// Get tile data
			let reader = self.reader.lock().await;
			let tile = reader.get_tile(&coord).await;
			drop(reader);

			// If tile data is not found, return a not found response
			if tile.is_err() {
				return Ok(None);
			}

			// If tile data is not found, return a not found response
			return if let Some(tile) = tile? {
				Ok(SourceResponse::new_some(
					tile.into_blob(self.compression)?,
					self.compression,
					&self.tile_mime,
				))
			} else {
				Ok(None)
			};
		} else if (parts[0] == "meta.json") || (parts[0] == "tiles.json") {
			// Get metadata
			let tile_json = self.build_tile_json().await?;

			return Ok(SourceResponse::new_some(
				tile_json,
				TileCompression::Uncompressed,
				"application/json",
			));
		}

		// If the request is unknown, return a not found response
		Ok(None)
	}

	#[context("building tilejson for tile source id='{}'", self.id)]
	async fn build_tile_json(&self) -> Result<Blob> {
		let reader = self.reader.lock().await;
		let mut tilejson = reader.tilejson().clone();
		tilejson.update_from_reader_parameters(reader.parameters());

		let tiles_url = self.prefix.join_as_string("{z}/{x}/{y}");
		tilejson.set_list("tiles", vec![tiles_url])?;

		Ok(tilejson.into())
	}
}

// Debug implementation for TileSource
impl Debug for TileSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileSource")
			.field("reader", &self.reader)
			.field("tile_mime", &self.tile_mime)
			.field("compression", &self.compression)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use rstest::rstest;
	use std::sync::Arc;
	use versatiles_container::{MockTilesReader, MockTilesReaderProfile, TilesRuntime};
	use versatiles_core::TileJSON;

	// Test the constructor function for TileSource
	#[tokio::test]
	async fn tile_container_from() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), "prefix")?;

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
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), "prefix")?;
		assert_eq!(
			format!("{container:?}"),
			"TileSource { reader: Mutex { data: MockTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [2: [0,1,2,3] (3x3), 3: [0,2,4,6] (5x5), 4: [0,0,15,15] (16x16), 5: [0,0,31,31] (32x32), 6: [0,0,63,63] (64x64)], tile_compression: Uncompressed, tile_format: PNG } } }, tile_mime: \"image/png\", compression: Uncompressed }"
		);
		Ok(())
	}

	// Test the get_data method of the TileSource
	#[rstest]
	#[case(
		"../testdata/berlin.mbtiles",
		"12/2200/1345",
		("vnd.mapbox-vector-tile", "[13.08283,52.33446,13.762245,52.6783]", [31, 139, 8, 0], 0, 14)
	)]
	#[case(
		"../testdata/berlin.pmtiles",
		"12/2200/1345",
		("vnd.mapbox-vector-tile", "[13.07373,52.321911,13.776855,52.683043]", [31, 139, 8, 0], 0, 14)
	)]
	#[case(
		"../testdata/berlin.vpl",
		"12/2200/1345",
		("vnd.mapbox-vector-tile", "[13.08283,52.33446,13.762245,52.6783]", [31, 139, 8, 0], 0, 14)
	)]
	#[tokio::test]
	async fn tile_container_get_data(
		#[case] filename: &str,
		#[case] coord: &str,
		#[case] expected_tile_json: (&str, &str, [u8; 4], u8, u8),
	) -> Result<()> {
		use TileCompression::*;

		async fn get_response(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
		) -> Result<Option<SourceResponse>> {
			container
				.get_data(&Url::from(url), &TargetCompression::from(compression))
				.await
		}

		async fn check_response(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
			mime_type: &str,
		) -> Result<Vec<u8>> {
			let response = get_response(container, url, compression).await?.unwrap();
			assert_eq!(response.mime, mime_type);
			Ok(response.blob.into_vec())
		}

		async fn check_status(container: &mut TileSource, url: &str) -> u16 {
			let response = get_response(container, url, Uncompressed).await;
			if response.is_err() {
				return 400;
			}
			if response.unwrap().is_none() { 404 } else { 200 }
		}

		let (exp_mime, exp_bounds, exp_header, exp_minzoom, exp_maxzoom) = expected_tile_json;

		let runtime = Arc::new(TilesRuntime::default());
		crate::register_readers(&runtime);
		let reader = runtime.registry().get_reader_from_str(filename).await?;
		let c = &mut TileSource::from(reader, "prefix")?;

		assert_eq!(
			&check_response(c, coord, Uncompressed, exp_mime).await?[0..4],
			exp_header
		);

		let tile_json = check_response(c, "meta.json", Uncompressed, "application/json").await?;
		let tile_json = TileJSON::try_from(tile_json)?.as_object();
		assert_eq!(tile_json.get_string("tile_format")?.unwrap(), exp_mime);
		assert_eq!(tile_json.get_array("bounds")?.unwrap().stringify(), exp_bounds);
		assert_eq!(tile_json.get_number("minzoom")?.unwrap() as u8, exp_minzoom);
		assert_eq!(tile_json.get_number("maxzoom")?.unwrap() as u8, exp_maxzoom);

		assert_eq!(check_status(c, "x/0/0.png").await, 400);
		assert_eq!(check_status(c, "-1/0/0.png").await, 400);
		assert_eq!(check_status(c, "0/0/-1.png").await, 400);
		assert_eq!(check_status(c, "16/0/0.png").await, 404);

		Ok(())
	}
}
