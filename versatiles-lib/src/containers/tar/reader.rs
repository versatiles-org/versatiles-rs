use crate::{
	containers::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait},
	shared::{
		decompress, extract_compression, extract_format, Blob, Compression, TileBBoxPyramid, TileCoord3, TileFormat,
	},
};
use anyhow::{anyhow, bail, ensure, Result};
use async_trait::async_trait;
use log;
use std::{collections::HashMap, fmt::Debug, fs::File, io::Read, os::unix::prelude::FileExt, path::Path};
use tar::{Archive, EntryType};

struct TarByteRange {
	offset: u64,
	length: u64,
}

pub struct TarTilesReader {
	meta: Option<Blob>,
	name: String,
	file: File,
	tile_map: HashMap<TileCoord3, TarByteRange>,
	parameters: TilesReaderParameters,
}

impl TarTilesReader {
	pub async fn open(path: &Path) -> Result<TilesReaderBox>
	where
		Self: Sized,
	{
		log::trace!("open {path:?}");

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

		Ok(Box::new(TarTilesReader {
			meta,
			name: path.to_str().unwrap().to_owned(),
			file,
			tile_map,
			parameters: TilesReaderParameters::new(tile_format.unwrap(), tile_compression.unwrap(), bbox_pyramid),
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
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		log::trace!("get_tile_data_original {:?}", coord);

		let range = self
			.tile_map
			.get(coord)
			.ok_or_else(|| anyhow!("tile {coord:?} not found"))?;

		let offset = range.offset;
		let length = range.length as usize;

		let mut buf: Vec<u8> = vec![0; length];
		self.file.read_exact_at(&mut buf, offset)?;

		Ok(Blob::from(buf))
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
		containers::{tests::make_test_file, MockTilesWriter, TilesWriterParameters, MOCK_BYTES_PBF},
		shared::decompress_brotli,
	};

	#[tokio::test]
	async fn reader() -> Result<()> {
		let temp_file = make_test_file(TileFormat::PBF, Compression::Brotli, 3, "tar").await?;

		// get tar reader
		let mut reader = TarTilesReader::open(&temp_file).await?;

		assert_eq!(format!("{:?}", reader), "TarTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64)], tile_compression: Brotli, tile_format: PBF } }");
		assert_eq!(reader.get_container_name(), "tar");
		assert!(reader.get_name().ends_with(temp_file.to_str().unwrap()));
		assert_eq!(reader.get_meta().await?, Some(Blob::from(b"dummy meta data".to_vec())));
		assert_eq!(format!("{:?}", reader.get_parameters()), "TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64)], tile_compression: Brotli, tile_format: PBF }");
		assert_eq!(reader.get_parameters().tile_compression, Compression::Brotli);
		assert_eq!(reader.get_parameters().tile_format, TileFormat::PBF);

		let tile = reader.get_tile_data(&TileCoord3::new(6, 2, 3)?).await?;
		assert_eq!(decompress_brotli(tile)?.as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}

	#[tokio::test]
	async fn all_compressions() -> Result<()> {
		async fn test_compression(compression: Compression) -> Result<()> {
			let temp_file = make_test_file(TileFormat::PBF, compression, 4, "tar").await?;

			// get tar reader
			let mut reader = TarTilesReader::open(&temp_file).await?;
			format!("{:?}", reader);

			let mut writer = MockTilesWriter::new_mock(TilesWriterParameters::new(TileFormat::PBF, compression));
			writer.write_from_reader(&mut reader).await?;
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

		let mut reader = TarTilesReader::open(&temp_file).await?;

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
			"tiles:\n   deep tiles probing is not implemented for this container format\n"
		);

		Ok(())
	}
}
