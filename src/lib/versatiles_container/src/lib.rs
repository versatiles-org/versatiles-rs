pub mod mbtiles;
pub mod tar_file;
mod traits;
pub mod versatiles;

use std::path::PathBuf;
pub use traits::*;
use versatiles_shared::{Result, TileConverterConfig};

pub async fn get_reader(filename: &str) -> Result<TileReaderBox> {
	let extension = filename.split('.').last().unwrap();

	let reader = match extension {
		"mbtiles" => mbtiles::TileReader::new(filename),
		"tar" => tar_file::TileReader::new(filename),
		"versatiles" => versatiles::TileReader::new(filename),
		_ => panic!("extension '{extension:?}' unknown"),
	};

	reader.await
}

pub fn get_converter(filename: &str, config: TileConverterConfig) -> TileConverterBox {
	let path = PathBuf::from(filename);
	let extension = path.extension().unwrap().to_str().unwrap();

	let converter = match extension {
		"mbtiles" => mbtiles::TileConverter::new(&path, config),
		"versatiles" => versatiles::TileConverter::new(&path, config),
		"tar" => tar_file::TileConverter::new(&path, config),
		_ => panic!("extension '{extension:?}' unknown"),
	};
	converter
}
