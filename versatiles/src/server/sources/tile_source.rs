use super::SourceResponse;
use crate::server::helpers::Url;
use anyhow::Result;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_lib::{
	containers::TileReaderBox,
	shared::{Compression, TargetCompression, TileCoord3, TileFormat},
};

// TileSource struct definition
#[derive(Clone)]
pub struct TileSource {
	pub id: String,
	pub prefix: String,
	pub json_info: String,
	reader: Arc<Mutex<TileReaderBox>>,
	pub tile_mime: String,
	pub compression: Compression,
}

impl TileSource {
	// Constructor function for creating a TileSource instance
	pub fn from(reader: TileReaderBox, id: &str, prefix: &str) -> Result<TileSource> {
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
			id: id.to_string(),
			prefix: prefix.to_string(),
			json_info,
			reader: Arc::new(Mutex::new(reader)),
			tile_mime,
			compression,
		})
	}

	// Retrieve the tile data as an HTTP response
	pub async fn get_data(&self, path: &[&str], _accept: &TargetCompression) -> Option<SourceResponse> {
		if path.len() == 3 {
			// Parse the tile coordinates
			let z = path[0].parse::<u8>();
			let x = path[1].parse::<u32>();
			let y: String = path[2].chars().take_while(|c| c.is_numeric()).collect();
			let y = y.parse::<u32>();

			// Check for parsing errors
			if x.is_err() || y.is_err() || z.is_err() {
				return None;
			}

			// Create a TileCoord3 instance
			let coord = TileCoord3::new(x.unwrap(), y.unwrap(), z.unwrap()).unwrap();

			log::debug!("get tile {:?} - {:?}", self.id, coord);

			// Get tile data
			let mut reader = self.reader.lock().await;
			let tile = reader.get_tile_data(&coord).await;
			drop(reader);

			// If tile data is not found, return a not found response
			if tile.is_err() {
				return None;
			}

			return SourceResponse::new_some(tile.unwrap(), &self.compression, &self.tile_mime);
		} else if (path[0] == "meta.json") || (path[0] == "tiles.json") {
			// Get metadata
			let reader = self.reader.lock().await;
			let meta_option = reader.get_meta().await.unwrap();
			drop(reader);

			// If metadata is empty, return a not found response
			meta_option.as_ref()?;

			return SourceResponse::new_some(meta_option.unwrap(), &Compression::None, "application/json");
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
	use anyhow::Result;
	use versatiles_lib::{
		containers::mock::{ReaderProfile, TileReader},
		shared::{
			Compression::{self, *},
			TargetCompression,
		},
	};

	// Test the constructor function for TileSource
	#[test]
	fn tile_container_from() -> Result<()> {
		let reader = TileReader::new_mock(ReaderProfile::PNG, 8);
		let container = TileSource::from(reader, "dummy id", "prefix")?;

		assert_eq!(container.id, "dummy id");
		assert_eq!(container.json_info, "{\"type\":\"dummy container\",\"format\":\"png\",\"compression\":\"none\",\"zoom_min\":0,\"zoom_max\":8,\"bbox\":[-180,-85.05112877980659,180,85.05112877980659]}");

		Ok(())
	}

	// Test the debug function
	#[test]
	fn debug() {
		let reader = TileReader::new_mock(ReaderProfile::PNG, 8);
		let container = TileSource::from(reader, "id", "prefix").unwrap();
		assert_eq!(format!("{container:?}"), "TileSource { reader: Mutex { data: TileReader:Dummy { parameters:  { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256), 5: [0,0,31,31] (1024), 6: [0,0,63,63] (4096), 7: [0,0,127,127] (16384), 8: [0,0,255,255] (65536)], decompressor: , flip_y: false, swap_xy: false, tile_compression: None, tile_format: PNG } } }, tile_mime: \"image/png\", compression: None }");
	}

	// Test the get_data method of the TileSource
	#[tokio::test]
	async fn tile_container_get_data() -> Result<()> {
		async fn check_response(container: &mut TileSource, url: &str, compression: Compression, mime_type: &str) -> Result<Vec<u8>> {
			let path: Vec<&str> = url.split('/').collect();
			let response = container.get_data(&path, &TargetCompression::from(compression)).await;
			assert!(response.is_some());

			let response = response.unwrap();
			assert_eq!(response.mime, mime_type);

			Ok(response.blob.as_vec())
		}

		async fn check_404(container: &mut TileSource, url: &str, compression: Compression) -> Result<bool> {
			let path: Vec<&str> = url.split('/').collect();
			let response = container.get_data(&path, &TargetCompression::from(compression)).await;
			assert!(response.is_none());
			Ok(true)
		}

		let c = &mut TileSource::from(TileReader::new_mock(ReaderProfile::PNG, 8), "id", "prefix")?;

		assert_eq!(&check_response(c, "0/0/0.png", None, "image/png").await?[0..6], b"\x89PNG\r\n");

		assert_eq!(
			&check_response(c, "meta.json", None, "application/json").await?[..],
			b"dummy meta data"
		);

		assert!(check_404(c, "x/0/0.png", None).await?);
		assert!(check_404(c, "-1/0/0.png", None).await?);
		assert!(check_404(c, "0/0/-1.png", None).await?);
		assert!(check_404(c, "0/0/1.png", None).await?);

		Ok(())
	}
}
