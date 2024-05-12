use crate::{
	container::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait},
	helper::decompress,
	types::{
		extract_compression, extract_format, Blob, ByteRange, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat,
	},
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::{
	collections::HashMap,
	fmt::Debug,
	fs::File,
	io::{BufReader, Read, Seek, SeekFrom},
	path::Path,
};
use tar::{Archive, EntryType};

pub struct TarTilesReader {
	meta: Option<Blob>,
	name: String,
	reader: BufReader<File>,
	tile_map: HashMap<TileCoord3, ByteRange>,
	parameters: TilesReaderParameters,
}

impl TarTilesReader {
	// Create a new TilesReader from a given filename
	pub fn open_path(path: &Path) -> Result<TilesReaderBox> {
		let mut reader = BufReader::new(File::open(path)?);
		let mut archive = Archive::new(&mut reader);

		let mut meta: Option<Blob> = None;
		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let mut tile_compression: Option<TileCompression> = None;
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
				if this_format.is_none() {
					continue;
				}
				let this_format = this_format.unwrap();

				let x = filename.parse::<u32>()?;

				if tile_format.is_none() {
					tile_format = Some(this_format);
				} else if tile_format.as_ref().unwrap() != &this_format {
					bail!("unknown filename {path_tmp_string:?}, can't detect format");
				}

				if tile_compression.is_none() {
					tile_compression = Some(this_compression);
				} else if tile_compression.as_ref().unwrap() != &this_compression {
					bail!("unknown filename {path_tmp_string:?}, can't detect compression");
				}

				let offset = entry.raw_file_position();
				let length = entry.size();

				let coord3 = TileCoord3::new(x, y, z)?;
				bbox_pyramid.include_coord(&coord3);
				tile_map.insert(coord3, ByteRange { offset, length });
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
						meta = Some(decompress(read_to_end(), &TileCompression::Gzip)?);
						continue;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						meta = Some(decompress(read_to_end(), &TileCompression::Brotli)?);
						continue;
					}
					&_ => {}
				};
			}

			log::warn!("unknown file in tar: {path_tmp_string:?}");
		}

		Ok(Box::new(TarTilesReader {
			meta,
			name: path.to_str().unwrap().to_string(),
			parameters: TilesReaderParameters::new(tile_format.unwrap(), tile_compression.unwrap(), bbox_pyramid),
			reader,
			tile_map,
		}))
	}
}

#[async_trait]
impl TilesReaderTrait for TarTilesReader {
	fn get_container_name(&self) -> &str {
		"tar"
	}
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}
	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		log::trace!("get_tile_data_original {:?}", coord);

		let range = self.tile_map.get(coord);

		if let Some(range) = range {
			let mut buffer = vec![0; range.length as usize];

			self.reader.seek(SeekFrom::Start(range.offset))?;
			self.reader.read_exact(&mut buffer)?;

			Ok(Some(Blob::from(buffer)))
		} else {
			Ok(None)
		}
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl Debug for TarTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TarTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::{
		container::{
			make_test_file,
			mock::{MockTilesWriter, MOCK_BYTES_PBF},
		},
		helper::decompress_gzip,
	};

	#[tokio::test]
	async fn reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::PBF, TileCompression::Gzip, 3, "tar").await?;

		// get tar reader
		let mut reader = TarTilesReader::open_path(&temp_file)?;

		assert_eq!(format!("{:?}", reader), "TarTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64)], tile_compression: Gzip, tile_format: PBF } }");
		assert_eq!(reader.get_container_name(), "tar");
		assert!(reader.get_name().ends_with(temp_file.to_str().unwrap()));
		assert_eq!(reader.get_meta()?, Some(Blob::from(b"dummy meta data".to_vec())));
		assert_eq!(format!("{:?}", reader.get_parameters()), "TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64)], tile_compression: Gzip, tile_format: PBF }");
		assert_eq!(reader.get_parameters().tile_compression, TileCompression::Gzip);
		assert_eq!(reader.get_parameters().tile_format, TileFormat::PBF);

		let tile = reader.get_tile_data(&TileCoord3::new(6, 2, 3)?).await?.unwrap();
		assert_eq!(decompress_gzip(tile)?.as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}

	#[tokio::test]
	async fn all_compressions() -> Result<()> {
		async fn test_compression(compression: TileCompression) -> Result<()> {
			let temp_file = make_test_file(TileFormat::PBF, compression, 2, "tar").await?;

			// get tar reader
			let mut reader = TarTilesReader::open_path(&temp_file)?;
			format!("{:?}", reader);

			let mut writer = MockTilesWriter::new_mock();
			writer.write_from_reader(&mut reader).await?;
			Ok(())
		}

		test_compression(TileCompression::None).await?;
		test_compression(TileCompression::Gzip).await?;
		test_compression(TileCompression::Brotli).await?;
		Ok(())
	}

	// Test tile fetching
	#[tokio::test]
	async fn probe() -> Result<()> {
		use crate::helper::pretty_print::PrettyPrint;

		let temp_file = make_test_file(TileFormat::PBF, TileCompression::Gzip, 4, "tar").await?;

		let mut reader = TarTilesReader::open_path(&temp_file)?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n   deep container probing is not implemented for this container format\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await,
			"tiles:\n   deep tiles probing is not implemented for this container format\n"
		);

		Ok(())
	}
}
