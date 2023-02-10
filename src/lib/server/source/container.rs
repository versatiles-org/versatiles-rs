use crate::{
	container::TileReaderBox,
	helper::*,
	server::{ok_data, ok_not_found, ServerSourceTrait},
};
use enumset::EnumSet;
use hyper::{Body, Response, Result};

pub struct TileContainer {
	reader: TileReaderBox,
	tile_mime: String,
	precompression: Precompression,
}
impl TileContainer {
	pub fn from(reader: TileReaderBox) -> Box<TileContainer> {
		let parameters = reader.get_parameters();
		let precompression = *parameters.get_tile_precompression();

		let tile_mime = match parameters.get_tile_format() {
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

		Box::new(TileContainer {
			reader,
			tile_mime,
			precompression,
		})
	}
}
impl ServerSourceTrait for TileContainer {
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}

	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Result<Response<Body>> {
		if path.len() == 3 {
			// get tile

			let z = path[0].parse::<u64>().unwrap();
			let y = path[1].parse::<u64>().unwrap();
			let x = path[2].parse::<u64>().unwrap();

			let tile = self.reader.get_tile_data(&TileCoord3::new(x, y, z));

			if tile.is_none() {
				return ok_not_found();
			}

			let mut data = tile.unwrap();

			if accept.contains(self.precompression) {
				return ok_data(data, &self.precompression, &self.tile_mime);
			}

			data = decompress(data, &self.precompression);
			return ok_data(data, &Precompression::Uncompressed, &self.tile_mime);
		} else if path[0] == "meta.json" {
			// get meta
			let meta = self.reader.get_meta();

			if meta.is_empty() {
				return ok_not_found();
			}

			let mime = "application/json";

			if accept.contains(Precompression::Brotli) {
				return ok_data(compress_brotli(meta), &Precompression::Brotli, mime);
			}

			if accept.contains(Precompression::Gzip) {
				return ok_data(compress_gzip(meta), &Precompression::Gzip, mime);
			}

			return ok_data(meta, &Precompression::Uncompressed, mime);
		}

		// unknown request;
		ok_not_found()
	}
}
