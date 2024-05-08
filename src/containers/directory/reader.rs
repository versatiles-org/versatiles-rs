use crate::{
	containers::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait},
	shared::{
		decompress, extract_compression, extract_format, Blob, Compression, TileBBoxPyramid, TileCoord3, TileFormat,
	},
};
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use std::{
	collections::HashMap,
	fmt::Debug,
	fs,
	path::{Path, PathBuf},
};

pub struct DirectoryTilesReader {
	meta: Option<Blob>,
	dir: PathBuf,
	tile_map: HashMap<TileCoord3, PathBuf>,
	parameters: TilesReaderParameters,
}

impl DirectoryTilesReader {
	pub async fn open(dir: &Path) -> Result<TilesReaderBox>
	where
		Self: Sized,
	{
		log::trace!("read {dir:?}");

		ensure!(dir.is_absolute(), "path {dir:?} must be absolute");
		ensure!(dir.exists(), "path {dir:?} does not exist");
		ensure!(dir.is_dir(), "path {dir:?} is not a directory");

		let mut meta: Option<Blob> = None;
		let mut tile_map = HashMap::new();
		let mut tile_format: Option<TileFormat> = None;
		let mut tile_compression: Option<Compression> = None;
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		for result1 in fs::read_dir(dir)? {
			// z level
			if result1.is_err() {
				continue;
			}
			let entry1 = result1?;
			let name1 = entry1.file_name().into_string().unwrap();
			let numeric1 = name1.parse::<u8>();
			if numeric1.is_ok() {
				let z = numeric1?;

				for result2 in fs::read_dir(entry1.path())? {
					// x level
					if result2.is_err() {
						continue;
					}
					let entry2 = result2?;
					let name2 = entry2.file_name().into_string().unwrap();
					let numeric2 = name2.parse::<u32>();
					if numeric2.is_err() {
						continue;
					}
					let x = numeric2?;

					for result3 in fs::read_dir(entry2.path())? {
						// y level
						if result3.is_err() {
							continue;
						}
						let entry3 = result3?;
						let mut filename = entry3.file_name().into_string().unwrap();
						let this_comp = extract_compression(&mut filename);
						let this_form = extract_format(&mut filename);

						let numeric3 = filename.parse::<u32>();
						if numeric3.is_err() {
							continue;
						}
						let y = numeric3?;

						if tile_format.is_none() {
							tile_format = Some(this_form);
						} else if tile_format != Some(this_form) {
							bail!("unknown filename {filename:?}, can't detect tile format");
						}

						if tile_compression.is_none() {
							tile_compression = Some(this_comp);
						} else if tile_compression != Some(this_comp) {
							bail!("unknown filename {filename:?}, can't detect tile compression");
						}

						let coord3 = TileCoord3::new(x, y, z)?;
						bbox_pyramid.include_coord(&coord3);
						tile_map.insert(coord3, entry3.path());
					}
				}
			} else {
				match name1.as_str() {
					"meta.json" | "tiles.json" | "metadata.json" => {
						meta = Some(Self::read(&entry1.path())?);
						continue;
					}
					"meta.json.gz" | "tiles.json.gz" | "metadata.json.gz" => {
						meta = Some(decompress(Self::read(&entry1.path())?, &Compression::Gzip)?);
						continue;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						meta = Some(decompress(Self::read(&entry1.path())?, &Compression::Brotli)?);
						continue;
					}
					&_ => {}
				};
			}
		}

		Ok(Box::new(DirectoryTilesReader {
			meta,
			dir: dir.to_path_buf(),
			tile_map,
			parameters: TilesReaderParameters::new(
				tile_format.expect("tile format must be specified"),
				tile_compression.expect("tile compression must be specified"),
				bbox_pyramid,
			),
		}))
	}

	fn read(path: &Path) -> Result<Blob> {
		Ok(Blob::from(fs::read(path)?))
	}
}

#[async_trait]
impl TilesReaderTrait for DirectoryTilesReader {
	fn get_container_name(&self) -> &str {
		"directory"
	}
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	fn override_compression(&mut self, tile_compression: Compression) {
		self.parameters.tile_compression = tile_compression;
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		log::trace!("get_tile_data_original {:?}", coord);

		if let Some(path) = self.tile_map.get(coord) {
			Self::read(path)
		} else {
			bail!("tile {:?} not found", coord);
		}
	}
	fn get_name(&self) -> &str {
		self.dir.to_str().unwrap()
	}
}

impl Debug for DirectoryTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DirectoryTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::TempDir;
	use std::fs::{self};

	#[tokio::test]
	async fn test_tile_reader_new() -> Result<()> {
		let dir = TempDir::new()?;

		fs::create_dir_all(dir.path().join("1/2"))?;
		fs::write(dir.path().join(".DS_Store"), "")?;
		fs::write(dir.path().join("1/2/3.png"), "test tile data")?;
		fs::write(dir.path().join("meta.json"), "test meta data")?;

		let mut reader = DirectoryTilesReader::open(&dir).await?;

		assert_eq!(reader.get_meta().await?.unwrap().as_str(), "test meta data");

		let coord = TileCoord3::new(2, 3, 1)?;
		let tile_data = reader.get_tile_data(&coord).await;
		assert!(tile_data.is_ok());
		assert_eq!(tile_data?, Blob::from("test tile data"));

		// Test for non-existent tile
		let coord = TileCoord3::new(2, 2, 1)?; // Assuming these coordinates do not exist
		assert!(reader.get_tile_data(&coord).await.is_err());

		return Ok(());
	}
}
