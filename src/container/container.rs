use std::path::PathBuf;

pub enum TileType {
	PBF,
	PNG,
	JPG,
	WEBP,
}
pub enum TileCompression {
	None,
	Gzip,
	Brotli,
}

pub trait Reader {
	fn load(filename: &PathBuf) -> Box<dyn Reader>
	where
		Self: Sized,
	{
		panic!("not implemented: load");
	}
	fn get_tile_type(&self) -> TileType {
		panic!("not implemented: get_tile_type");
	}
	fn get_tile_compression(&self) -> TileCompression {
		panic!("not implemented: get_tile_compression");
	}
	fn get_meta(&self) -> Vec<u8> {
		panic!("not implemented: get_meta");
	}
}

pub trait Converter {
	fn convert_from(filename: &PathBuf, container: Box<dyn Reader>) -> std::io::Result<()> {
		panic!("not implemented: convert_from");
	}
}
