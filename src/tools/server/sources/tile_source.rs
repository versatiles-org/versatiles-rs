use super::{super::utils::Url, SourceResponse};
use crate::{
	container::TilesReader,
	types::{TileCompression, TileCoord3, TileFormat},
	utils::TargetCompression,
};
use anyhow::Result;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;

// TileSource struct definition
#[derive(Clone)]
pub struct TileSource {
	pub prefix: Url,
	pub json_info: String,
	reader: Arc<Mutex<Box<dyn TilesReader>>>,
	pub tile_mime: String,
	pub compression: TileCompression,
}

impl TileSource {
	// Constructor function for creating a TileSource instance
	pub fn from(reader: Box<dyn TilesReader>, prefix: Url) -> Result<TileSource> {
		let parameters = reader.get_parameters();
		let compression = parameters.tile_compression;

		// Determine the MIME type based on the tile format
		let tile_mime = match parameters.tile_format {
			// Various tile formats with their corresponding MIME types
			TileFormat::BIN => "application/octet-stream",
			TileFormat::PNG => "image/png",
			TileFormat::JPG => "image/jpeg",
			TileFormat::WEBP => "image/webp",
			TileFormat::AVIF => "image/avif",
			TileFormat::SVG => "image/svg+xml",
			TileFormat::PBF => "application/x-protobuf",
			TileFormat::GEOJSON => "application/geo+json",
			TileFormat::TOPOJSON => "application/topo+json",
			TileFormat::JSON => "application/json",
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
			prefix,
			json_info,
			reader: Arc::new(Mutex::new(reader)),
			tile_mime,
			compression,
		})
	}

	pub async fn get_id(&self) -> String {
		let reader = self.reader.lock().await;
		return reader.get_name().to_owned();
	}

	// Retrieve the tile data as an HTTP response
	pub async fn get_data(&self, url: &Url, _accept: &TargetCompression) -> Option<SourceResponse> {
		let parts: Vec<String> = url.as_vec();

		if parts.len() >= 3 {
			// Parse the tile coordinates
			let z = parts[0].parse::<u8>();
			let x = parts[1].parse::<u32>();
			let y: String = parts[2].chars().take_while(|c| c.is_numeric()).collect();
			let y = y.parse::<u32>();

			// Check for parsing errors
			if x.is_err() || y.is_err() || z.is_err() {
				return None;
			}

			// Create a TileCoord3 instance
			let coord = TileCoord3::new(x.unwrap(), y.unwrap(), z.unwrap()).unwrap();

			log::debug!("get tile {} - {:?}", self.prefix, coord);

			// Get tile data
			let mut reader = self.reader.lock().await;
			let tile = reader.get_tile_data(&coord).await;
			drop(reader);

			// If tile data is not found, return a not found response
			if tile.is_err() {
				return None;
			}

			let tile = tile.unwrap();

			// If tile data is not found, return a not found response
			return if let Some(tile) = tile {
				SourceResponse::new_some(tile, &self.compression, &self.tile_mime)
			} else {
				None
			};
		} else if (parts[0] == "meta.json") || (parts[0] == "tiles.json") {
			// Get metadata
			let reader = self.reader.lock().await;
			let meta_option = reader.get_meta().unwrap();
			drop(reader);

			// If metadata is empty, return a not found response
			meta_option.as_ref()?;

			return SourceResponse::new_some(
				meta_option.unwrap(),
				&TileCompression::None,
				"application/json",
			);
		}

		// If the request is unknown, return a not found response
		None
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
	use crate::container::{MockTilesReader, MockTilesReaderProfile};
	use anyhow::Result;

	// Test the constructor function for TileSource
	#[test]
	fn tile_container_from() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), Url::new("prefix"))?;

		assert_eq!(container.prefix.str, "/prefix");
		assert_eq!(container.json_info, "{\"type\":\"dummy_container\",\"format\":\"png\",\"compression\":\"none\",\"zoom_min\":0,\"zoom_max\":4,\"bbox\":[-180,-85.05112877980659,180,85.05112877980659]}");

		Ok(())
	}

	// Test the debug function
	#[test]
	fn debug() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		let container = TileSource::from(reader.boxed(), Url::new("prefix")).unwrap();
		assert_eq!(format!("{container:?}"), "TileSource { reader: Mutex { data: MockTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256)], tile_compression: None, tile_format: PNG } } }, tile_mime: \"image/png\", compression: None }");
		Ok(())
	}

	// Test the get_data method of the TileSource
	#[tokio::test]
	async fn tile_container_get_data() -> Result<()> {
		async fn check_response(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
			mime_type: &str,
		) -> Result<Vec<u8>> {
			let response = container
				.get_data(&Url::new(url), &TargetCompression::from(compression))
				.await;
			assert!(response.is_some());

			let response = response.unwrap();
			assert_eq!(response.mime, mime_type);

			Ok(response.blob.into_vec())
		}

		async fn check_404(
			container: &mut TileSource,
			url: &str,
			compression: TileCompression,
		) -> Result<bool> {
			let response = container
				.get_data(&Url::new(url), &TargetCompression::from(compression))
				.await;
			assert!(response.is_none());
			Ok(true)
		}

		let c = &mut TileSource::from(
			MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?.boxed(),
			Url::new("prefix"),
		)?;

		assert_eq!(
			&check_response(c, "0/0/0.png", TileCompression::None, "image/png").await?[0..6],
			b"\x89PNG\r\n"
		);

		assert_eq!(
			&check_response(c, "meta.json", TileCompression::None, "application/json").await?[..],
			b"dummy meta data"
		);

		assert!(check_404(c, "x/0/0.png", TileCompression::None).await?);
		assert!(check_404(c, "-1/0/0.png", TileCompression::None).await?);
		assert!(check_404(c, "0/0/-1.png", TileCompression::None).await?);
		assert!(check_404(c, "0/0/1.png", TileCompression::None).await?);

		Ok(())
	}
}
