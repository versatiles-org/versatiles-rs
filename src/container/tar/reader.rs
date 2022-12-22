use crate::container;

pub struct Reader;
impl container::Reader for Reader {
	#![allow(unused_variables)]
	fn get_level_bbox(&self, level: u64) -> (u64, u64, u64, u64) {
		panic!();
	}
	fn get_meta(&self) -> &[u8] {
		panic!();
	}
	fn get_maximum_zoom(&self) -> u64 {
		panic!();
	}
	fn get_minimum_zoom(&self) -> u64 {
		panic!();
	}
	fn get_tile_compression(&self) -> container::TileCompression {
		panic!();
	}
	fn get_tile_format(&self) -> container::TileFormat {
		panic!();
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		panic!();
	}
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		panic!();
	}
	fn load(filename: &std::path::PathBuf) -> std::io::Result<Box<dyn container::Reader>>
	where
		Self: Sized,
	{
		panic!();
	}
}
