use std::path::PathBuf;

#[derive(PartialEq, Clone)]
pub enum TileType {
	PBF,
	PNG,
	JPG,
	WEBP,
}
#[derive(PartialEq, Clone)]
pub enum TileCompression {
	None,
	Gzip,
	Brotli,
}

pub trait Reader {
	fn load(filename: &PathBuf) -> std::io::Result<Box<dyn Reader>>
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
	fn get_minimum_level(&self) -> u64 {
		panic!("not implemented: get_minimum_level")
	}
	fn get_maximum_level(&self) -> u64 {
		panic!("not implemented: get_maximum_level");
	}
	fn get_minimum_col(&self, level: u64) -> u64 {
		panic!("not implemented: get_minimum_col")
	}
	fn get_maximum_col(&self, level: u64) -> u64 {
		panic!("not implemented: get_maximum_col");
	}
	fn get_minimum_row(&self, level: u64) -> u64 {
		panic!("not implemented: get_minimum_row")
	}
	fn get_maximum_row(&self, level: u64) -> u64 {
		panic!("not implemented: get_maximum_row");
	}
	fn get_tile_uncompressed(&self, level: u64, row: u64, col: u64) -> std::io::Result<Vec<u8>> {
		panic!("not implemented: get_tile_uncompressed");
	}
	fn get_tile_raw(&self, level: u64, row: u64, col: u64) -> std::io::Result<Vec<u8>> {
		panic!("not implemented: get_tile_raw");
	}
}

pub trait Converter {
	fn convert_from(filename: &PathBuf, container: Box<dyn Reader>) -> std::io::Result<()> {
		panic!("not implemented: convert_from");
	}
}
