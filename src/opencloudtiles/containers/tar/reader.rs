use crate::opencloudtiles::{
	containers::abstract_container,
	types::{TileBBoxPyramide, TileData, TileFormat, TileReaderParameters},
};
use std::{collections::HashMap, fs::File, os::unix::prelude::FileExt};
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
	file: File,
	tile_map: HashMap<TileKey, TarByteRange>,
	parameters: TileReaderParameters,
}
impl abstract_container::TileReader for TileReader {
	fn load(filename: &std::path::PathBuf) -> Box<dyn abstract_container::TileReader>
	where
		Self: Sized,
	{
		let file = File::open(filename).unwrap();
		let mut archive = Archive::new(&file);

		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let mut bbox_pyramide = TileBBoxPyramide::new_empty();

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
			bbox_pyramide.include_tile(z as u64, x, y);
		}

		return Box::new(TileReader {
			file,
			tile_map,
			parameters: TileReaderParameters::new(tile_format.unwrap(), bbox_pyramide),
		});
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return &self.parameters;
	}
	fn get_meta(&self) -> &[u8] {
		return &[0u8; 0];
	}
	fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<TileData> {
		let key = TileKey {
			z: level as u8,
			y: row,
			x: col,
		};
		let range = self.tile_map.get(&key);

		if range.is_none() {
			return None;
		}

		let offset = range.unwrap().offset;
		let length = range.unwrap().length as usize;

		let mut buf: Vec<u8> = Vec::new();
		buf.resize(length, 0);

		self.file.read_exact_at(&mut buf, offset).unwrap();

		return Some(buf);
	}
}
