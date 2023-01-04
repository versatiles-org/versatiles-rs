use crate::opencloudtiles::{
	containers::abstract_container::TileReaderBox,
	types::{Compression, TileFormat},
};
use enumset::EnumSet;
use hyper::{Body, Response, Result};

pub trait ServerSourceTrait: Send + Sync {
	fn get_name(&self) -> &str;
	fn get_data(&self, path: &[String], accept: EnumSet<Compression>) -> Result<Response<Body>>;
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

pub struct ServerSourceTileReader {
	reader: TileReaderBox,
	tile_format: TileFormat,
	precompression: Compression,
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

	fn get_data(&self, path: &[String], accept: EnumSet<Compression>) -> Result<Response<Body>> {
		if path.len() == 3 {
			// get tile
			todo!()
		} else if path[0] == "meta.json" {
			// get meta
			todo!()
		} else {
			// unknown request;
			todo!()
		}
	}
}
