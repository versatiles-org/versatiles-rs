use crate::opencloudtiles::{
	abstract_classes,
	types::{TileBBoxPyramide, TileFormat, TileReaderParameters},
};
use std::{collections::HashMap, fs::File};
use tar::{Archive, EntryType};

#[derive(PartialEq, Eq, Hash)]
struct TileKey {
	z: u8,
	y: u64,
	x: u64,
}

struct TarByteRange {
	offset: u64,
	length: u64,
}

pub struct TileReader {
	tile_map: HashMap<TileKey, TarByteRange>,
	parameters: Option<TileReaderParameters>,
}
impl abstract_classes::TileReader for TileReader {
	fn load(filename: &std::path::PathBuf) -> Box<dyn abstract_classes::TileReader>
	where
		Self: Sized,
	{
		let file = File::open(filename).unwrap();
		let mut archive = Archive::new(file);

		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let level_bbox = TileBBoxPyramide::new();

		for file in archive.entries().unwrap() {
			let file = file.unwrap();
			let header = file.header();
			if header.entry_type() != EntryType::Regular {
				continue;
			}

			let path = file.path().unwrap();
			let fullname: Vec<&str> = path.to_str().unwrap().split('/').collect();
			// expecting something like:
			// "./6/21/34.png" -> [".", "6", "21", "34.png"]

			assert_eq!(fullname.len(), 4);
			assert_eq!(fullname[0], ".");

			let z = fullname[1].parse::<u8>().unwrap();
			let y = fullname[2].parse::<u64>().unwrap();
			let filename: Vec<&str> = fullname[3].split(".").collect();
			let x = filename[0].parse::<u64>().unwrap();

			let extension = filename[1..].join(".");
			let this_tile_format = Some(match extension.as_str() {
				"png" => TileFormat::PNG,
				"jpg" => TileFormat::JPG,
				"jpeg" => TileFormat::JPG,
				"webp" => TileFormat::WEBP,
				"pbf" => TileFormat::PBF,
				"pbf.gz" => TileFormat::PBFGzip,
				"pbf.br" => TileFormat::PBFBrotli,
				_ => panic!("unknown extension {}", extension),
			});

			if tile_format.is_none() {
				tile_format = this_tile_format;
			} else {
				assert_eq!(tile_format, this_tile_format, "unknown filename {:?}", path);
			}

			let offset = file.raw_file_position();
			let length = file.size();

			tile_map.insert(TileKey { z, y, x }, TarByteRange { offset, length });

			//println!("{:?} {}", fullname, fullname.len());

			// Inspect metadata about the file
			//println!("{:?} {} {}", path, offset, length);
		}
		let zoom_min: u64 = 100;
		let zoom_max: u64 = 0;
		let parameters = Some(TileReaderParameters::new(
			zoom_min,
			zoom_max,
			tile_format.unwrap(),
			level_bbox,
		));
		return Box::new(TileReader {
			tile_map,
			parameters,
		});
	}
}
