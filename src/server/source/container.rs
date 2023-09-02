use crate::{
	containers::TileReaderBox,
	server::{make_result, ServerSourceResult, ServerSourceTrait},
	shared::{Compression, Result, TargetCompression, TileCoord3, TileFormat},
};
use async_trait::async_trait;
use std::fmt::Debug;

// TileContainer struct definition
pub struct TileContainer {
	reader: TileReaderBox,
	tile_mime: String,
	compression: Compression,
}

impl TileContainer {
	// Constructor function for creating a TileContainer instance
	pub fn from(reader: TileReaderBox) -> Result<Box<TileContainer>> {
		let parameters = reader.get_parameters()?;
		let compression = *parameters.get_tile_compression();

		// Determine the MIME type based on the tile format
		let tile_mime = match parameters.get_tile_format() {
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

		Ok(Box::new(TileContainer {
			reader,
			tile_mime,
			compression,
		}))
	}
}

#[async_trait]
impl ServerSourceTrait for TileContainer {
	// Get the name of the tile container
	fn get_name(&self) -> Result<String> {
		Ok(self.reader.get_name()?.to_owned())
	}

	// Get information about the tile container as a JSON string
	fn get_info_as_json(&self) -> Result<String> {
		let parameters = self.reader.get_parameters()?;
		let bbox_pyramid = parameters.get_bbox_pyramid();

		let tile_format = format!("{:?}", parameters.get_tile_format()).to_lowercase();
		let tile_compression = format!("{:?}", parameters.get_tile_compression()).to_lowercase();

		Ok(format!(
			"{{ \"container\":\"{}\", \"format\":\"{}\", \"compression\":\"{}\", \"zoom_min\":{}, \"zoom_max\":{}, \"bbox\":{:?} }}",
			self.reader.get_container_name()?,
			tile_format,
			tile_compression,
			bbox_pyramid.get_zoom_min().unwrap(),
			bbox_pyramid.get_zoom_max().unwrap(),
			bbox_pyramid.get_geo_bbox(),
		))
	}

	// Retrieve the tile data as an HTTP response
	async fn get_data(&mut self, path: &[&str], _accept: &TargetCompression) -> Option<ServerSourceResult> {
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
			let coord = TileCoord3::new(x.unwrap(), y.unwrap(), z.unwrap());

			// Get tile data
			let tile = self.reader.get_tile_data(&coord).await;

			// If tile data is not found, return a not found response
			if tile.is_err() {
				return None;
			}

			return make_result(tile.unwrap(), &self.compression, &self.tile_mime);
		} else if (path[0] == "meta.json") || (path[0] == "tiles.json") {
			// Get metadata
			let meta = self.reader.get_meta().await.unwrap();

			// If metadata is empty, return a not found response
			if meta.is_empty() {
				return None;
			}

			return make_result(meta, &Compression::None, &String::from("application/json"));
		}

		// If the request is unknown, return a not found response
		None
	}
}

// Debug implementation for TileContainer
impl Debug for TileContainer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileContainer")
			.field("reader", &self.reader)
			.field("tile_mime", &self.tile_mime)
			.field("compression", &self.compression)
			.finish()
	}
}

// Test module
#[cfg(test)]
mod tests {
	use super::TileContainer;
	use crate::{
		containers::dummy::{ReaderProfile, TileReader},
		server::ServerSourceTrait,
		shared::{
			Compression::{self, *},
			Result, TargetCompression,
		},
	};

	// Test the constructor function for TileContainer
	#[test]
	fn tile_container_from() -> Result<()> {
		let reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		let container = TileContainer::from(reader)?;

		assert_eq!(container.get_name().unwrap(), "dummy name");

		let expected_info = r#"{ "container":"dummy container", "format":"png", "compression":"none", "zoom_min":0, "zoom_max":8, "bbox":[-180.0, -85.05112877980659, 180.0, 85.05112877980659] }"#;
		assert_eq!(container.get_info_as_json().unwrap(), expected_info);

		Ok(())
	}

	// Test the debug function
	#[test]
	fn debug() {
		let reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		let container = TileContainer::from(reader).unwrap();
		let debug = format!("{container:?}");
		assert!(debug.starts_with("TileContainer { reader: TileReader:Dummy {"));
	}

	// Test the get_data method of the TileContainer
	#[tokio::test]
	async fn tile_container_get_data() -> Result<()> {
		async fn check_response(
			container: &mut TileContainer, url: &str, compression: Compression, mime_type: &str,
		) -> Result<Vec<u8>> {
			let path: Vec<&str> = url.split('/').collect();
			let response = container.get_data(&path, &TargetCompression::from(compression)).await;
			assert!(response.is_some());

			let response = response.unwrap();
			assert_eq!(response.mime, mime_type);

			Ok(response.blob.as_vec())
		}

		async fn check_404(container: &mut TileContainer, url: &str, compression: Compression) -> Result<bool> {
			let path: Vec<&str> = url.split('/').collect();
			let response = container.get_data(&path, &TargetCompression::from(compression)).await;
			assert!(response.is_none());
			Ok(true)
		}

		let c = &mut TileContainer::from(TileReader::new_dummy(ReaderProfile::PngFast, 8))?;

		assert_eq!(
			&check_response(c, "0/0/0.png", None, "image/png").await?[0..6],
			b"\x89PNG\r\n"
		);

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
