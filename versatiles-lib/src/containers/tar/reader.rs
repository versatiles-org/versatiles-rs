use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	create_error,
	shared::{
		decompress, extract_compression, extract_format, Blob, Compression, TileBBoxPyramid, TileCoord3, TileFormat,
		TileReaderParameters,
	},
};
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use log;
use std::{
	collections::HashMap,
	env::{self},
	fmt::Debug,
	fs::File,
	io::Read,
	os::unix::prelude::FileExt,
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
	meta: Option<Blob>,
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
	async fn new(filename: &str) -> Result<TileReaderBox>
	where
		Self: Sized,
	{
		log::trace!("new {}", filename);

		let path = env::current_dir().unwrap().join(filename);

		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		let file = File::open(path)?;
		let mut archive = Archive::new(&file);

		let mut meta: Option<Blob> = None;
		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let mut tile_compression: Option<Compression> = None;
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
				let y = path_vec[1].parse::<u32>()?;

				let mut filename: String = String::from(path_vec[2]);
				let this_compression = extract_compression(&mut filename);
				let this_format = extract_format(&mut filename);
				let x = filename.parse::<u32>()?;

				if tile_format.is_none() {
					tile_format = Some(this_format);
				} else if tile_format.as_ref().unwrap() != &this_format {
					return create_error!("unknown filename {path_tmp_string:?}, can't detect format");
				}

				if tile_compression.is_none() {
					tile_compression = Some(this_compression);
				} else if tile_compression.as_ref().unwrap() != &this_compression {
					return create_error!("unknown filename {path_tmp_string:?}, can't detect compression");
				}

				let offset = entry.raw_file_position();
				let length = entry.size();

				let coord3 = TileCoord3::new(x, y, z)?;
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
						meta = Some(read_to_end());
						continue;
					}
					"meta.json.gz" | "tiles.json.gz" | "metadata.json.gz" => {
						meta = Some(decompress(read_to_end(), &Compression::Gzip)?);
						continue;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						meta = Some(decompress(read_to_end(), &Compression::Brotli)?);
						continue;
					}
					&_ => {}
				};
			}

			log::warn!("unknown file in tar: {path_tmp_string:?}");
		}

		Ok(Box::new(TileReader {
			meta,
			name: String::from(filename),
			file,
			tile_map,
			parameters: TileReaderParameters::new(tile_format.unwrap(), tile_compression.unwrap(), bbox_pyramid),
		}))
	}
	fn get_parameters(&self) -> Result<&TileReaderParameters> {
		Ok(&self.parameters)
	}
	fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters> {
		Ok(&mut self.parameters)
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob> {
		log::trace!("get_tile_data_original {:?}", coord);

		let range = self.tile_map.get(coord);
		if range.is_none() {
			bail!("tile {:?} not found", coord);
		}
		let range = range.unwrap();

		let offset = range.offset;
		let length = range.length as usize;

		let mut buf: Vec<u8> = vec![0; length];
		self.file.read_exact_at(&mut buf, offset)?;

		Ok(Blob::from(buf))
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
		mock::{ConverterProfile, TileConverter},
		tests::make_test_file,
	};

	#[tokio::test]
	async fn reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::PNG, Compression::Brotli, 4, "tar").await?;
		let temp_file = temp_file.to_str().unwrap();

		// get tar reader
		let mut reader = TileReader::new(temp_file).await?;

		assert_eq!(format!("{:?}", reader), "TileReader:Tar { parameters: Ok( { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256)], decompressor: UnBrotli, flip_y: false, swap_xy: false, tile_compression: Brotli, tile_format: PNG }) }");
		assert_eq!(reader.get_container_name()?, "tar");
		assert!(reader.get_name()?.ends_with(temp_file));
		assert_eq!(reader.get_meta().await?, Some(Blob::from(b"dummy meta data".to_vec())));
		assert_eq!(format!("{:?}", reader.get_parameters()?), " { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256)], decompressor: UnBrotli, flip_y: false, swap_xy: false, tile_compression: Brotli, tile_format: PNG }");
		assert_eq!(reader.get_tile_compression()?, &Compression::Brotli);
		assert_eq!(reader.get_tile_format()?, &TileFormat::PNG);

		let tile = reader.get_tile_data_original(&TileCoord3::new(12, 3, 4)?).await?;
		assert_eq!(tile, Blob::from( b"\x053\x80\x89PNG\x0d\x0a\x1a\x0a\x00\x00\x00\x0dIHDR\x00\x00\x01\x00\x00\x00\x01\x00\x01\x03\x00\x00\x00f\xbc:%\x00\x00\x00\x03PLTE\xaa\xd3\xdf\xcf\xec\xbc\xf5\x00\x00\x00\x1fIDATh\x81\xed\xc1\x01\x0d\x00\x00\x00\xc2\xa0\xf7Om\x0e7\xa0\x00\x00\x00\x00\x00\x00\x00\x00\xbe\x0d!\x00\x00\x01\x9a`\xe1\xd5\x00\x00\x00\x00IEND\xaeB`\x82\x03".to_vec()));

		Ok(())
	}

	#[tokio::test]
	async fn all_compressions() -> Result<()> {
		async fn test_compression(compression: Compression) -> Result<()> {
			let temp_file = make_test_file(TileFormat::PBF, compression, 4, "tar").await?;
			let temp_file = temp_file.to_str().unwrap();

			// get tar reader
			let mut reader = TileReader::new(temp_file).await?;
			reader.get_parameters_mut()?;
			format!("{:?}", reader);

			let mut converter = TileConverter::new_mock(ConverterProfile::Whatever, 4);
			converter.convert_from(&mut reader).await?;
			Ok(())
		}

		test_compression(Compression::None).await?;
		test_compression(Compression::Gzip).await?;
		test_compression(Compression::Brotli).await?;
		Ok(())
	}

	// Test tile fetching
	#[tokio::test]
	async fn probe() -> Result<()> {
		use crate::shared::PrettyPrint;

		let temp_file = make_test_file(TileFormat::PBF, Compression::Gzip, 4, "tar").await?;
		let temp_file = temp_file.to_str().unwrap();

		let mut reader = TileReader::new(temp_file).await?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n   deep container probing is not implemented for this container format\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await,
			"tiles:\n   deep tile probing is not implemented for this container format\n"
		);

		Ok(())
	}
}
