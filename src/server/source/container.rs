use crate::{
	containers::TileReaderBox,
	server::{ok_data, ok_not_found, ServerSourceTrait},
	shared::{compress_brotli, compress_gzip, decompress, Compression, Result, TileCoord3, TileFormat},
};
use async_trait::async_trait;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
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
	async fn get_data(&mut self, path: &[&str], accept: EnumSet<Compression>) -> Response<Full<Bytes>> {
		if path.len() == 3 {
			// Parse the tile coordinates
			let z = path[0].parse::<u8>();
			let x = path[1].parse::<u64>();
			let y: String = path[2].chars().take_while(|c| c.is_numeric()).collect();
			let y = y.parse::<u64>();

			// Check for parsing errors
			if x.is_err() || y.is_err() || z.is_err() {
				return ok_not_found();
			}

			// Create a TileCoord3 instance
			let coord = TileCoord3::new(x.unwrap(), y.unwrap(), z.unwrap());

			// Get tile data
			let tile = self.reader.get_tile_data(&coord).await;

			// If tile data is not found, return a not found response
			if tile.is_none() {
				return ok_not_found();
			}

			let mut data = tile.unwrap();

			// If the accepted compression matches the container's compression, return the data
			if accept.contains(self.compression) {
				return ok_data(data, &self.compression, &self.tile_mime);
			}

			// Decompress the data and return it
			data = decompress(data, &self.compression).unwrap();
			return ok_data(data, &Compression::None, &self.tile_mime);
		} else if (path[0] == "meta.json") || (path[0] == "tiles.json") {
			// Get metadata
			let meta = self.reader.get_meta().await.unwrap();

			// If metadata is empty, return a not found response
			if meta.is_empty() {
				return ok_not_found();
			}

			let mime = "application/json";

			// Return the compressed metadata based on the accepted compression
			if accept.contains(Compression::Brotli) {
				return ok_data(compress_brotli(meta).unwrap(), &Compression::Brotli, mime);
			}

			if accept.contains(Compression::Gzip) {
				return ok_data(compress_gzip(meta).unwrap(), &Compression::Gzip, mime);
			}

			return ok_data(meta, &Compression::None, mime);
		}

		// If the request is unknown, return a not found response
		ok_not_found()
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
			Result,
		},
	};
	use axum::body::Bytes;
	use enumset::EnumSet;
	use hyper::body::HttpBody;

	// Test the constructor function for TileContainer
	#[test]
	fn tile_container_from() -> Result<()> {
		let reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		let container = TileContainer::from(reader)?;

		assert_eq!(container.get_name().unwrap(), "dummy name");

		let expected_info = r#"{ "container":"dummy container", "format":"png", "compression":"none", "zoom_min":0, "zoom_max":8, "bbox":[-180.0, -85.05113, 180.0, 85.05112] }"#;
		assert_eq!(container.get_info_as_json().unwrap(), expected_info);

		Ok(())
	}

	// Test the debug function
	#[test]
	fn debug() {
		let reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		let container = TileContainer::from(reader).unwrap();
		let debug = format!("{container:?}");
		println!("{debug}");
		assert!(debug.starts_with("TileContainer { reader: TileReader:Dummy {"));
	}

	// Test the get_data method of the TileContainer
	#[tokio::test]
	async fn tile_container_get_data() -> Result<()> {
		async fn check_response(
			container: &mut TileContainer, url: &str, compression: Compression, status: u16, content_type: &str,
		) -> Result<Bytes> {
			let path: Vec<&str> = url.split("/").collect();
			let mut response = container.get_data(&path, EnumSet::only(compression)).await;
			assert_eq!(response.status(), status);
			assert_eq!(response.headers().get("content-type").unwrap().to_str()?, content_type);
			let body: Bytes = response.data().await.unwrap()?;
			Ok(body)
		}

		async fn check_404(container: &mut TileContainer, url: &str, compression: Compression) -> Result<bool> {
			let path: Vec<&str> = url.split("/").collect();
			let mut response = container.get_data(&path, EnumSet::only(compression)).await;
			assert_eq!(response.status(), 404, "for url {url}");
			let body: Bytes = response.data().await.unwrap()?;
			assert_eq!(body, "Not Found");
			Ok(true)
		}

		let c = &mut TileContainer::from(TileReader::new_dummy(ReaderProfile::PngFast, 8))?;

		assert_eq!(
			&check_response(c, "0/0/0.png", None, 200, "image/png").await?[0..6],
			b"\x89PNG\r\n"
		);

		assert_eq!(
			&check_response(c, "meta.json", None, 200, "application/json").await?[..],
			b"dummy meta data"
		);

		assert_eq!(
			&check_response(c, "meta.json", Brotli, 200, "application/json").await?[..],
			[11, 7, 128, 100, 117, 109, 109, 121, 32, 109, 101, 116, 97, 32, 100, 97, 116, 97, 3]
		);

		assert_eq!(
			&check_response(c, "meta.json", Gzip, 200, "application/json").await?[..],
			[
				31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 75, 41, 205, 205, 173, 84, 200, 77, 45, 73, 84, 72, 73, 44, 73, 4, 0,
				191, 165, 147, 231, 15, 0, 0, 0
			]
		);

		assert!(check_404(c, "x/0/0.png", None).await?);
		assert!(check_404(c, "-1/0/0.png", None).await?);
		assert!(check_404(c, "0/0/-1.png", None).await?);
		assert!(check_404(c, "0/0/1.png", None).await?);

		Ok(())
	}
}
