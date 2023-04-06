use crate::{ok_data, ok_not_found, ServerSourceTrait};
use async_trait::async_trait;
use axum::{
	body::{Bytes, Full},
	response::Response,
};
use enumset::EnumSet;
use std::fmt::Debug;
use versatiles_container::TileReaderBox;
use versatiles_shared::{compress_brotli, compress_gzip, decompress, Precompression, TileCoord3, TileFormat};

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

#[async_trait]
impl ServerSourceTrait for TileContainer {
	fn get_name(&self) -> String {
		self.reader.get_name().to_owned()
	}
	fn get_info_as_json(&self) -> String {
		let parameters = self.reader.get_parameters();
		let bbox_pyramide = parameters.get_bbox_pyramide();

		let tile_format = format!("{:?}", parameters.get_tile_format()).to_lowercase();
		let tile_precompression = format!("{:?}", parameters.get_tile_precompression()).to_lowercase();

		format!(
			"{{ \"container\":\"{}\", \"format\":\"{}\", \"precompression\":\"{}\", \"zoom_min\":{}, \"zoom_max\":{}, \"bbox\":{:?} }}",
			self.reader.get_container_name(),
			tile_format,
			tile_precompression,
			bbox_pyramide.get_zoom_min().unwrap(),
			bbox_pyramide.get_zoom_max().unwrap(),
			bbox_pyramide.get_geo_bbox(),
		)
	}

	async fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Response<Full<Bytes>> {
		if path.len() == 3 {
			let z = path[0].parse::<u8>();
			let x = path[1].parse::<u64>();
			let y: String = path[2].chars().take_while(|c| c.is_numeric()).collect();
			let y = y.parse::<u64>();

			if x.is_err() || y.is_err() || z.is_err() {
				return ok_not_found();
			}

			let coord = TileCoord3::new(x.unwrap(), y.unwrap(), z.unwrap());

			// get tile
			let tile = self.reader.get_tile_data(&coord).await;

			if tile.is_none() {
				return ok_not_found();
			}

			let mut data = tile.unwrap();

			if accept.contains(self.precompression) {
				return ok_data(data, &self.precompression, &self.tile_mime);
			}

			data = decompress(data, &self.precompression).unwrap();
			return ok_data(data, &Precompression::Uncompressed, &self.tile_mime);
		} else if (path[0] == "meta.json") || (path[0] == "tiles.json") {
			// get meta
			let meta = self.reader.get_meta().await;

			if meta.is_empty() {
				return ok_not_found();
			}

			let mime = "application/json";

			if accept.contains(Precompression::Brotli) {
				return ok_data(compress_brotli(meta).unwrap(), &Precompression::Brotli, mime);
			}

			if accept.contains(Precompression::Gzip) {
				return ok_data(compress_gzip(meta).unwrap(), &Precompression::Gzip, mime);
			}

			return ok_data(meta, &Precompression::Uncompressed, mime);
		}

		// unknown request;
		ok_not_found()
	}
}

impl Debug for TileContainer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileContainer")
			.field("reader", &self.reader)
			.field("tile_mime", &self.tile_mime)
			.field("precompression", &self.precompression)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::TileContainer;
	use versatiles_container::dummy::{ReaderProfile, TileReader};

	#[test]
	fn tile_container_from() {
		let reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		let _container = TileContainer::from(reader);
	}
}
