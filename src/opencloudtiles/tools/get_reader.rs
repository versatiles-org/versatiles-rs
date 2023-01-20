use crate::opencloudtiles::container::{
	cloudtiles, mbtiles, tar_file, TileReaderBox, TileReaderTrait,
};

pub fn get_reader(filename: &str) -> TileReaderBox {
	let extension = filename.split('.').last().unwrap();

	let reader = match extension {
		"mbtiles" => mbtiles::TileReader::new(filename),
		"tar" => tar_file::TileReader::new(filename),
		"cloudtiles" => cloudtiles::TileReader::new(filename),
		_ => panic!("extension '{extension:?}' unknown"),
	};

	reader
}
