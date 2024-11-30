use super::{super::utils::Url, SourceResponse};
use anyhow::{ensure, Result};
use std::{collections::BTreeMap, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_core::{
	types::{Blob, TileCompression, TileCoord3, TileFormat, TilesReaderTrait},
	utils::{JsonValue, TargetCompression},
};

// TileSource struct definition
#[derive(Clone)]
pub struct TileSource {
	pub prefix: Url,
	pub id: String,
	pub json_info: String,
	reader: Arc<Mutex<Box<dyn TilesReaderTrait>>>,
	pub tile_mime: String,
	pub compression: TileCompression,
}

impl TileSource {
	// Constructor function for creating a TileSource instance
	pub fn from(reader: Box<dyn TilesReaderTrait>, id: &str) -> Result<TileSource> {
		use TileFormat::*;

		let parameters = reader.get_parameters();
		let compression = parameters.tile_compression;

		// Determine the MIME type based on the tile format
		let tile_mime = match parameters.tile_format {
			// Various tile formats with their corresponding MIME types
			BIN => "application/octet-stream",
			PNG => "image/png",
			JPG => "image/jpeg",
			WEBP => "image/webp",
			AVIF => "image/avif",
			SVG => "image/svg+xml",
			PBF => "application/x-protobuf",
			GEOJSON => "application/geo+json",
			TOPOJSON => "application/topo+json",
			JSON => "application/json",
		}
		.to_string();

		let bbox_pyramid = &parameters.bbox_pyramid;
		let tile_format = format!("{:?}", parameters.tile_format).to_lowercase();
		let tile_compression = format!("{:?}", parameters.tile_compression).to_lowercase();
		let json_info = format!(
			"{{\"type\":\"{}\",\"format\":\"{}\",\"compression\":\"{}\",\"zoom_min\":{},\"zoom_max\":{},\"bbox\":[{}]}}",
			reader.get_container_name(),
			tile_format,
			tile_compression,
			bbox_pyramid.get_zoom_min().unwrap(),
			bbox_pyramid.get_zoom_max().unwrap(),
			bbox_pyramid.get_geo_bbox().map(|f| f.to_string()).join(","),
		);

		Ok(TileSource {
			prefix: Url::new(&format!("/tiles/{id}/")).as_dir(),
			id: id.to_owned(),
			json_info,
			reader: Arc::new(Mutex::new(reader)),
			tile_mime,
			compression,
		})
	}

	pub async fn get_source_name(&self) -> String {
		let reader = self.reader.lock().await;
		reader.get_source_name().to_owned()
	}

	// Retrieve the tile data as an HTTP response
	pub async fn get_data(
		&self,
		url: &Url,
		_accept: &TargetCompression,
	) -> Result<Option<SourceResponse>> {
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

			log::debug!(
				"get tile, prefix: {}, coord: {}",
				self.prefix,
				coord.as_json()
			);

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
				Ok(SourceResponse::new_some(
					tile,
					&self.compression,
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
				&TileCompression::Uncompressed,
				"application/json",
			));
		}

		// If the request is unknown, return a not found response
		Ok(None)
	}

	async fn build_tile_json(&self) -> Result<Blob> {
		let tiles_url = format!("{}{{z}}/{{x}}/{{y}}", self.prefix.as_string());

		let reader = self.reader.lock().await;
		let meta = reader.get_meta()?;
		let parameters = reader.get_parameters();

		let bbox = parameters.bbox_pyramid.get_geo_bbox();
		let zoom_min = parameters.bbox_pyramid.get_zoom_min().unwrap();
		let zoom_max = parameters.bbox_pyramid.get_zoom_max().unwrap();

		let (tile_format, tile_type) = match parameters.tile_format {
			TileFormat::AVIF => ("image", "avif"),
			TileFormat::BIN => ("unknown", "bin"),
			TileFormat::GEOJSON => ("vector", "geojson"),
			TileFormat::JPG => ("image", "jpeg"),
			TileFormat::JSON => ("unknown", "json"),
			TileFormat::PBF => ("vector", "pbf"),
			TileFormat::PNG => ("image", "png"),
			TileFormat::SVG => ("image", "svg"),
			TileFormat::TOPOJSON => ("vector", "topojson"),
			TileFormat::WEBP => ("image", "webp"),
		};

		drop(reader);

		let mut tilejson = JsonValue::Object(BTreeMap::from([
			(String::from("bounds"), JsonValue::from(bbox.to_vec())),
			(String::from("format"), JsonValue::from(tile_format)),
			(String::from("maxzoom"), JsonValue::from(zoom_max)),
			(String::from("minzoom"), JsonValue::from(zoom_min)),
			(String::from("name"), JsonValue::from(self.id.as_str())),
			(String::from("tilejson"), JsonValue::from("3.0.0")),
			(String::from("tiles"), JsonValue::from(vec![tiles_url])),
			(String::from("type"), JsonValue::from(tile_type)),
			(
				String::from("center"),
				JsonValue::from(vec![
					(bbox[0] + bbox[2]) / 2.,
					(bbox[1] + bbox[3]) / 2.,
					(zoom_min + 2).min(zoom_max) as f64,
				]),
			),
		]));

		if let Some(meta) = meta {
			tilejson.object_assign(JsonValue::parse_blob(&meta)?)?
		}

		Ok(Blob::from(tilejson.stringify()))
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
	#[test]
	fn tile_container_from() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), "prefix")?;

		assert_eq!(container.prefix.str, "/tiles/prefix/");
		assert_eq!(container.json_info, "{\"type\":\"dummy_container\",\"format\":\"png\",\"compression\":\"uncompressed\",\"zoom_min\":2,\"zoom_max\":3,\"bbox\":[-180,-79.17133464081944,45,66.51326044311185]}");

		Ok(())
	}

	// Test the debug function
	#[test]
	fn debug() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), "prefix")?;
		assert_eq!(format!("{container:?}"), "TileSource { reader: Mutex { data: MockTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [2: [0,1,2,3] (9), 3: [0,2,4,6] (25)], tile_compression: Uncompressed, tile_format: PNG } } }, tile_mime: \"image/png\", compression: Uncompressed }");
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

		async fn check_error_400(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
		) -> Result<bool> {
			let response = container
				.get_data(&Url::new(url), &TargetCompression::from(compression))
				.await;
			assert!(response.is_err());
			Ok(true)
		}

		async fn check_error_404(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
		) -> Result<bool> {
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
			String::from_utf8(
				check_response(c, "meta.json", Uncompressed, "application/json").await?
			)?,
			"{\"bounds\":[-180,-79.17133464081944,45,66.51326044311185],\"center\":[-67.5,-6.329037098853796,3],\"format\":\"image\",\"maxzoom\":3,\"minzoom\":2,\"name\":\"prefix\",\"tilejson\":\"3.0.0\",\"tiles\":[\"/tiles/prefix/{z}/{x}/{y}\"],\"type\":\"dummy\"}"
		);

		assert!(check_error_400(c, "x/0/0.png", Uncompressed).await?);
		assert!(check_error_400(c, "-1/0/0.png", Uncompressed).await?);
		assert!(check_error_400(c, "0/0/-1.png", Uncompressed).await?);
		assert!(check_error_404(c, "0/0/1.png", Uncompressed).await?);

		Ok(())
	}
}
