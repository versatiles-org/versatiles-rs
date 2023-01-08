use crate::{
	opencloudtiles::{
		container::{cloudtiles, mbtiles, tar, TileReaderBox, TileReaderTrait},
		
	},
};

pub fn get_reader(filename: &str) -> TileReaderBox {
	let extension = filename.split('.').last().unwrap();

	let reader = match extension {
		"mbtiles" => mbtiles::TileReader::new(filename),
		"tar" => tar::TileReader::new(filename),
		"cloudtiles" => cloudtiles::TileReader::new(filename),
		_ => panic!("extension '{:?}' unknown", extension),
	};

	reader
}
