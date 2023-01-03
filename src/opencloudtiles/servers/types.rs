use crate::opencloudtiles::containers::abstract_container::TileReaderBox;
use enumset::{EnumSet, EnumSetType};

#[derive(EnumSetType)]
pub enum ServerEncoding {
	Uncompressed,
	Gzip,
	Brotli,
}

pub trait ServerSourceTrait {
	fn get_name(&self) -> &str;
	fn get_data(&self, path: &[String], accept: EnumSet<ServerEncoding>) -> (ServerEncoding, &[u8]);
}

pub type ServerSourceBox = Box<dyn ServerSourceTrait>;

pub struct ServerSourceTileReader {
	reader: TileReaderBox,
}
impl ServerSourceTileReader {
	pub fn from_reader(reader: TileReaderBox) -> Box<ServerSourceTileReader> {
		Box::new(ServerSourceTileReader { reader })
	}
}
impl ServerSourceTrait for ServerSourceTileReader {
	fn get_name(&self) -> &str {
		self.reader.get_name()
	}

	fn get_data(&self, path: &[String], accept: EnumSet<ServerEncoding>) -> (ServerEncoding, &[u8]) {
		todo!()
	}
}
