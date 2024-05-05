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
	env,
	fmt::Debug,
	fs,
	path::{Path, PathBuf},
};

pub struct TileReader {
	meta: Option<Blob>,
	path: PathBuf,
	tile_map: HashMap<TileCoord3, PathBuf>,
	parameters: TileReaderParameters,
}

impl TileReader {
	fn read(path: &Path) -> Result<Blob> {
		Ok(Blob::from(fs::read(path)?))
	}
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
		let path = env::current_dir().unwrap().join(filename);
		log::trace!("read {:?}", path);

		ensure!(path.is_dir(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		let mut meta: Option<Blob> = None;
		let mut tile_map = HashMap::new();
		let mut tile_form: Option<TileFormat> = None;
		let mut tile_comp: Option<Compression> = None;
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		for result1 in fs::read_dir(&path)? {
			// z level
			if result1.is_err() {
				continue;
			}
			let entry1 = result1.unwrap();
			let name1 = entry1.file_name().into_string().unwrap();
			let numeric1 = name1.parse::<u8>();
			if numeric1.is_ok() {
				let z = numeric1.unwrap();

				for result2 in fs::read_dir(entry1.path())? {
					// x level
					if result2.is_err() {
						continue;
					}
					let entry2 = result2.unwrap();
					let name2 = entry2.file_name().into_string().unwrap();
					let numeric2 = name2.parse::<u32>();
					if numeric2.is_err() {
						continue;
					}
					let x = numeric2.unwrap();

					for result3 in fs::read_dir(entry2.path())? {
						// y level
						if result3.is_err() {
							continue;
						}
						let entry3 = result3.unwrap();
						let mut filename = entry3.file_name().into_string().unwrap();
						let this_comp = extract_compression(&mut filename);
						let this_form = extract_format(&mut filename);

						let numeric3 = filename.parse::<u32>();
						if numeric3.is_err() {
							continue;
						}
						let y = numeric3.unwrap();

						if tile_form.is_none() {
							tile_form = Some(this_form);
						} else if tile_form.as_ref().unwrap() != &this_form {
							return create_error!("unknown filename {filename:?}, can't detect format");
						}

						if tile_comp.is_none() {
							tile_comp = Some(this_comp);
						} else if tile_comp.as_ref().unwrap() != &this_comp {
							return create_error!("unknown filename {filename:?}, can't detect compression");
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

		Ok(Box::new(TileReader {
			meta,
			path,
			tile_map,
			parameters: TileReaderParameters::new(tile_form.unwrap(), tile_comp.unwrap(), bbox_pyramid),
		}))
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		&self.parameters
	}
	fn get_parameters_mut(&mut self) -> &mut TileReaderParameters {
		&mut self.parameters
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta.clone())
	}
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob> {
		log::trace!("get_tile_data_original {:?}", coord);

		let path = self.tile_map.get(coord);
		if path.is_none() {
			bail!("tile {:?} not found", coord);
		}

		Ok(Self::read(path.unwrap())?)
	}
	fn get_name(&self) -> Result<&str> {
		Ok(self.path.to_str().unwrap())
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:Directory")
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

		let mut reader = TileReader::new(dir.to_str().unwrap()).await.unwrap();

		assert_eq!(reader.get_meta().await?.unwrap().as_str(), "test meta data");

		let coord = TileCoord3::new(2, 3, 1).unwrap();
		let tile_data = reader.get_tile_data_original(&coord).await;
		assert!(tile_data.is_ok());
		assert_eq!(tile_data.unwrap(), Blob::from("test tile data"));

		// Test for non-existent tile
		let coord = TileCoord3::new(2, 2, 1).unwrap(); // Assuming these coordinates do not exist
		assert!(reader.get_tile_data_original(&coord).await.is_err());

		return Ok(());
	}
}
