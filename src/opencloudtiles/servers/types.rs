use crate::opencloudtiles::{
	containers::TileReaderBox,
	types::{Blob, Precompression, TileFormat},
};
use enumset::EnumSet;
use hyper::{Body, Response, Result, StatusCode};

pub trait ServerSourceTrait: Send + Sync {
	fn get_name(&self) -> &str;
	fn get_data(&self, path: &[&str], accept: EnumSet<Precompression>) -> Result<Response<Body>>;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

pub struct ServerSourceTileReader {
	reader: TileReaderBox,
	tile_format: TileFormat,
	precompression: Precompression,
}
impl ServerSourceTileReader {
	pub fn from_reader(reader: TileReaderBox) -> Box<ServerSourceTileReader> {
		let parameters = reader.get_parameters();
		let tile_format = parameters.get_tile_format().clone();
		let precompression = parameters.get_tile_precompression().clone();
		Box::new(ServerSourceTileReader {
			reader,
			tile_format,
			precompression,
		})
	}
}
impl ServerSourceTrait for ServerSourceTileReader {
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}

	fn get_data(&self, path: &[&str], _accept: EnumSet<Precompression>) -> Result<Response<Body>> {
		let ok_data =
			|data: Blob, _precompression: &Precompression, _mime: &str| -> Result<Response<Body>> {
				return Ok(Response::builder()
					.status(StatusCode::OK)
					.body(data.to_vec().into())
					.unwrap());
			};

		let ok_not_found = || -> Result<Response<Body>> {
			return Ok(Response::builder()
				.status(StatusCode::NOT_FOUND)
				.body("Not Found".into())
				.unwrap());
		};

		if path.len() == 3 {
			// get tile
			todo!()
		} else if path[0] == "meta.json" {
			// get meta
			let meta = self.reader.get_meta();
			println!("bytes.len() {}", meta.len());

			if meta.len() == 0 {
				return ok_not_found();
			}

			return ok_data(meta, &Precompression::Uncompressed, "application/json");
		} else {
			// unknown request;
			return ok_not_found();
		}
	}
}
