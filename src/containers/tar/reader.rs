use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	shared::{
		decompress, Blob, Compression, Error, Result, TileBBoxPyramid, TileCoord3, TileFormat, TileReaderParameters,
	},
};
use async_trait::async_trait;
use log::trace;
use std::{
	collections::HashMap, env::current_dir, fmt::Debug, fs::File, io::Read, os::unix::prelude::FileExt, path::Path,
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
#[async_trait]
impl TileReaderTrait for TileReader {
	fn get_container_name(&self) -> Result<&str> {
		Ok("tar")
	}
	async fn new(path: &str) -> Result<TileReaderBox>
	where
		Self: Sized,
	{
		trace!("new {}", path);
		let mut filename = current_dir()?;
		filename.push(Path::new(path));

		assert!(filename.exists(), "file {filename:?} does not exist");
		assert!(filename.is_absolute(), "path {filename:?} must be absolute");

		filename = filename.canonicalize()?;

		let file = File::open(filename)?;
		let mut archive = Archive::new(&file);

		let mut meta = Blob::empty();
		let mut tile_map = HashMap::new();
		let mut tile_form: Option<TileFormat> = None;
		let mut tile_comp: Option<Compression> = None;
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		for entry in archive.entries()? {
			let mut entry = entry?;
			let header = entry.header();
			if header.entry_type() != EntryType::Regular {
				continue;
			}

			let path = entry.path()?.clone();
			let mut path_tmp: Vec<&str> = path.iter().map(|s| s.to_str().unwrap()).collect();

			if path_tmp[0] == "." {
				path_tmp.remove(0);
			}

			let path_tmp_string = path_tmp.join("/");
			drop(path);
			let path_vec: Vec<&str> = path_tmp_string.split('/').collect();

			if path_vec.len() == 3 {
				let z = path_vec[0].parse::<u8>()?;
				let y = path_vec[1].parse::<u64>()?;

				let mut filename: Vec<&str> = path_vec[2].split('.').collect();
				let x = filename[0].parse::<u64>()?;

				let mut extension = filename.pop().unwrap();
				let this_comp = match extension {
					"gz" => {
						extension = filename.pop().unwrap();
						Compression::Gzip
					}
					"br" => {
						extension = filename.pop().unwrap();
						Compression::Brotli
					}
					_ => Compression::None,
				};

				let this_form = match extension {
					"png" => TileFormat::PNG,
					"jpg" => TileFormat::JPG,
					"jpeg" => TileFormat::JPG,
					"webp" => TileFormat::WEBP,
					"pbf" => TileFormat::PBF,
					_ => panic!("unknown extension for {path_vec:?}"),
				};

				if tile_form.is_none() {
					tile_form = Some(this_form);
				} else if tile_form.as_ref().unwrap() != &this_form {
					return Err(Error::new(&format!(
						"unknown filename {path_tmp_string:?}, can't detect format"
					)));
				}

				if tile_comp.is_none() {
					tile_comp = Some(this_comp);
				} else if tile_comp.as_ref().unwrap() != &this_comp {
					return Err(Error::new(&format!(
						"unknown filename {path_tmp_string:?}, can't detect compression"
					)));
				}

				let offset = entry.raw_file_position();
				let length = entry.size();

				let coord3 = TileCoord3 { x, y, z };
				bbox_pyramid.include_coord(&coord3);
				tile_map.insert(coord3, TarByteRange { offset, length });
				continue;
			}

			let mut read_to_end = || {
				let mut blob: Vec<u8> = Vec::new();
				entry.read_to_end(&mut blob).unwrap();
				Blob::from(blob)
			};

			if path_vec.len() == 1 {
				match path_vec[0] {
					"meta.json" | "tiles.json" | "metadata.json" => {
						meta = read_to_end();
						continue;
					}
					"meta.json.gz" | "tiles.json.gz" | "metadata.json.gz" => {
						meta = decompress(read_to_end(), &Compression::Gzip)?;
						continue;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						meta = decompress(read_to_end(), &Compression::Brotli)?;
						continue;
					}
					&_ => {}
				};
			}

			return Err(Error::new(&format!("unknown file in tar: {path_tmp_string:?}")));
		}

		Ok(Box::new(TileReader {
			meta,
			name: path.to_string(),
			file,
			tile_map,
			parameters: TileReaderParameters::new(tile_form.unwrap(), tile_comp.unwrap(), bbox_pyramid),
		}))
	}
	fn get_parameters(&self) -> Result<&TileReaderParameters> {
		Ok(&self.parameters)
	}
	fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters> {
		Ok(&mut self.parameters)
	}
	async fn get_meta(&self) -> Result<Blob> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data(&mut self, coord_in: &TileCoord3) -> Option<Blob> {
		trace!("get_tile_data {:?}", coord_in);

		let coord: TileCoord3 = if self.get_parameters().unwrap().get_vertical_flip() {
			coord_in.flip_vertically()
		} else {
			coord_in.to_owned()
		};

		let range = self.tile_map.get(&coord)?;

		let offset = range.offset;
		let length = range.length as usize;

		let mut buf: Vec<u8> = Vec::new();
		buf.resize(length, 0);

		self.file.read_exact_at(&mut buf, offset).unwrap();

		Some(Blob::from(buf))
	}
	fn get_name(&self) -> Result<&str> {
		Ok(&self.name)
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:Tar")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::containers::{
		dummy::{ConverterProfile, TileConverter},
		tests::make_test_file,
	};

	#[tokio::test]
	async fn all_compressions() -> Result<()> {
		async fn test_compression(compression: Compression) -> Result<()> {
			let file = make_test_file(TileFormat::PBF, compression, 4, "tar").await?;

			// get tar reader
			let mut reader = TileReader::new(file.to_str().unwrap()).await?;
			reader.get_parameters_mut()?;
			format!("{:?}", reader);

			let mut converter = TileConverter::new_dummy(ConverterProfile::Whatever, 4);
			converter.convert_from(&mut reader).await?;
			Ok(())
		}

		test_compression(Compression::None).await?;
		test_compression(Compression::Gzip).await?;
		test_compression(Compression::Brotli).await?;
		Ok(())
	}
}
