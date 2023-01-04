use crate::opencloudtiles::{
	containers::abstract_container::{self, TileReaderBox, TileReaderTrait},
	types::{Blob, Precompression, TileBBoxPyramide, TileCoord3, TileFormat, TileReaderParameters},
};
use std::{
	collections::HashMap, fmt::Debug, fs::File, os::unix::prelude::FileExt, path::PathBuf,
	str::from_utf8,
};
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
	meta: Blob,
	name: String,
	file: File,
	tile_map: HashMap<TileCoord3, TarByteRange>,
	parameters: TileReaderParameters,
}
impl abstract_container::TileReaderTrait for TileReader {
	fn from_file(filename: &PathBuf) -> TileReaderBox
	where
		Self: Sized,
	{
		let file = File::open(filename).unwrap();
		let mut archive = Archive::new(&file);

		let mut tile_map = HashMap::new();
		let mut tile_form: Option<TileFormat> = None;
		let mut tile_comp: Option<Precompression> = None;
		let mut bbox_pyramide = TileBBoxPyramide::new_empty();

		for file in archive.entries().unwrap() {
			let file = file.unwrap();
			let header = file.header();
			if header.entry_type() != EntryType::Regular {
				continue;
			}

			let path = file.path().unwrap();
			let fullname: Vec<&str> = path.iter().map(|s| s.to_str().unwrap()).collect();

			// expecting something like:
			// "./6/21/34.png" -> [".", "6", "21", "34.png"]
			assert_eq!(fullname.len(), 4);
			assert_eq!(fullname[0], ".");

			let z = fullname[1].parse::<u64>().unwrap();
			let y = fullname[2].parse::<u64>().unwrap();

			let mut filename: Vec<&str> = fullname[3].split(".").collect();
			let x = filename[0].parse::<u64>().unwrap();

			let mut extension = filename.pop().unwrap();
			let this_comp = match extension {
				"gz" => {
					extension = filename.pop().unwrap();
					Precompression::Gzip
				}
				"br" => {
					extension = filename.pop().unwrap();
					Precompression::Brotli
				}
				_ => Precompression::Uncompressed,
			};

			let this_form = match extension {
				"png" => TileFormat::PNG,
				"jpg" => TileFormat::JPG,
				"jpeg" => TileFormat::JPG,
				"webp" => TileFormat::WEBP,
				"pbf" => TileFormat::PBF,
				_ => panic!("unknown extension for {:?}", fullname),
			};

			if tile_form.is_none() {
				tile_form = Some(this_form);
			} else {
				assert_eq!(
					tile_form.as_ref().unwrap(),
					&this_form,
					"unknown filename {:?}",
					path
				);
			}

			if tile_comp.is_none() {
				tile_comp = Some(this_comp);
			} else {
				assert_eq!(
					tile_comp.as_ref().unwrap(),
					&this_comp,
					"unknown filename {:?}",
					path
				);
			}

			let offset = file.raw_file_position();
			let length = file.size();

			let coord3 = TileCoord3 { z, y, x };
			bbox_pyramide.include_coord(&coord3);
			tile_map.insert(coord3, TarByteRange { offset, length });
		}

		return Box::new(TileReader {
			meta: Blob::empty(),
			name: filename.to_string_lossy().to_string(),
			file,
			tile_map,
			parameters: TileReaderParameters::new(
				tile_form.unwrap(),
				tile_comp.unwrap(),
				bbox_pyramide,
			),
		});
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return &self.parameters;
	}
	fn get_meta(&self) -> Blob {
		return self.meta.clone();
	}
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<Blob> {
		let range = self.tile_map.get(&coord);

		if range.is_none() {
			return None;
		}

		let offset = range.unwrap().offset;
		let length = range.unwrap().length as usize;

		let mut buf: Vec<u8> = Vec::new();
		buf.resize(length, 0);

		self.file.read_exact_at(&mut buf, offset).unwrap();

		return Some(Blob::from_vec(buf));
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:Tar")
			.field("meta", &from_utf8(self.get_meta().as_slice()).unwrap())
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
