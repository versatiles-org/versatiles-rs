use super::{super::utils::Url, SourceResponse};
use anyhow::{Result, ensure};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_core::{Blob, TileCompression, TileCoord3, TilesReaderTrait, utils::TargetCompression};

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
	pub fn from(reader: Box<dyn TilesReaderTrait>, id: &str) -> Result<TileSource> {
		let parameters = reader.parameters();
		let tile_mime = parameters.tile_format.as_mime_str().to_string();
		let compression = parameters.tile_compression;

		Ok(TileSource {
			prefix: Url::new(&format!("/tiles/{id}/")).as_dir(),
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
	pub async fn get_data(&self, url: &Url, _accept: &TargetCompression) -> Result<Option<SourceResponse>> {
		let parts: Vec<String> = url.as_vec();

		if parts.len() >= 3 {
			// Parse the tile coordinates
			let z = parts[0].parse::<u8>();
			let x = parts[1].parse::<u32>();
			let y: String = parts[2].chars().take_while(|c| c.is_numeric()).collect();
			let y = y.parse::<u32>();

			// Check for parsing errors
			ensure!(z.is_ok(), "value for z is not a number");
			ensure!(x.is_ok(), "value for x is not a number");
			ensure!(y.is_ok(), "value for y is not a number");

			// Create a TileCoord3 instance
			let coord = TileCoord3::new(x?, y?, z?)?;

			log::debug!("get tile, prefix: {}, coord: {}", self.prefix, coord.as_json());

			// Get tile data
			let reader = self.reader.lock().await;
			let tile = reader.get_tile_data(&coord).await;
			drop(reader);

			// If tile data is not found, return a not found response
			if tile.is_err() {
				return Ok(None);
			}

			// If tile data is not found, return a not found response
			return if let Some(tile) = tile? {
				Ok(SourceResponse::new_some(tile, &self.compression, &self.tile_mime))
			} else {
				Ok(None)
			};
		} else if (parts[0] == "meta.json") || (parts[0] == "tiles.json") {
			// Get metadata
			let tile_json = self.build_tile_json().await?;

			return Ok(SourceResponse::new_some(
				tile_json,
				&TileCompression::Uncompressed,
				"application/json",
			));
		}

		// If the request is unknown, return a not found response
		Ok(None)
	}

	async fn build_tile_json(&self) -> Result<Blob> {
		let reader = self.reader.lock().await;
		let mut tilejson = reader.tilejson().clone();
		tilejson.update_from_reader_parameters(reader.parameters());

		let tiles_url = format!("{}{{z}}/{{x}}/{{y}}", self.prefix.as_string());
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
	use versatiles_container::{MockTilesReader, MockTilesReaderProfile};

	// Test the constructor function for TileSource
	#[tokio::test]
	async fn tile_container_from() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), "prefix")?;

		assert_eq!(container.prefix.str, "/tiles/prefix/");
		assert_eq!(
			container.build_tile_json().await?.as_str(),
			"{\"bounds\":[-180,-79.171335,45,66.51326],\"maxzoom\":3,\"minzoom\":2,\"tile_content\":\"raster\",\"tile_format\":\"image/png\",\"tile_schema\":\"rgb\",\"tilejson\":\"3.0.0\",\"tiles\":[\"/tiles/prefix/{z}/{x}/{y}\"],\"type\":\"dummy\"}"
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
			"TileSource { reader: Mutex { data: MockTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [2: [0,1,2,3] (9), 3: [0,2,4,6] (25)], tile_compression: Uncompressed, tile_format: PNG } } }, tile_mime: \"image/png\", compression: Uncompressed }"
		);
		Ok(())
	}

	// Test the get_data method of the TileSource
	#[tokio::test]
	async fn tile_container_get_data() -> Result<()> {
		use TileCompression::*;

		async fn check_response(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
			mime_type: &str,
		) -> Result<Vec<u8>> {
			let response = container
				.get_data(&Url::new(url), &TargetCompression::from(compression))
				.await?;
			assert!(response.is_some());

			let response = response.unwrap();
			assert_eq!(response.mime, mime_type);

			Ok(response.blob.into_vec())
		}

		async fn check_error_400(container: &mut TileSource, url: &str, compression: TileCompression) -> Result<bool> {
			let response = container
				.get_data(&Url::new(url), &TargetCompression::from(compression))
				.await;
			assert!(response.is_err());
			Ok(true)
		}

		async fn check_error_404(container: &mut TileSource, url: &str, compression: TileCompression) -> Result<bool> {
			let response = container
				.get_data(&Url::new(url), &TargetCompression::from(compression))
				.await?;
			assert!(response.is_none());
			Ok(true)
		}

		let c = &mut TileSource::from(
			MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?.boxed(),
			"prefix",
		)?;

		assert_eq!(
			&check_response(c, "0/0/0.png", Uncompressed, "image/png").await?[0..6],
			b"\x89PNG\r\n"
		);

		assert_eq!(
			String::from_utf8(check_response(c, "meta.json", Uncompressed, "application/json").await?)?,
			"{\"bounds\":[-180,-79.171335,45,66.51326],\"maxzoom\":3,\"minzoom\":2,\"tile_content\":\"raster\",\"tile_format\":\"image/png\",\"tile_schema\":\"rgb\",\"tilejson\":\"3.0.0\",\"tiles\":[\"/tiles/prefix/{z}/{x}/{y}\"],\"type\":\"dummy\"}"
		);

		assert!(check_error_400(c, "x/0/0.png", Uncompressed).await?);
		assert!(check_error_400(c, "-1/0/0.png", Uncompressed).await?);
		assert!(check_error_400(c, "0/0/-1.png", Uncompressed).await?);
		assert!(check_error_404(c, "0/0/1.png", Uncompressed).await?);

		Ok(())
	}
}
