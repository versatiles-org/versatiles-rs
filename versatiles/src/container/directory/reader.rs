//! This module provides functionality for reading tile data from a directory structure.
//!
//! The `DirectoryTilesReader` struct is the primary component of this module, offering methods to open a directory, read metadata, and fetch tile data based on coordinates.
//!
//! ## Directory Structure
//! The directory should follow a specific structure to organize the tiles:
//! ```text
//! <root>/<z>/<x>/<y>.<format>[.<compression>]
//! ```
//! - `<z>`: Zoom level (directory)
//! - `<x>`: Tile X coordinate (directory)
//! - `<y>.<format>[.<compression>]`: Tile Y coordinate with the tile format and optional compression type as the file extension
//!
//! Example:
//! ```text
//! /tiles/1/2/3.png
//! /tiles/1/2/4.jpg.br
//! /tiles/meta.json
//! ```
//!
//! ## Features
//! - Supports multiple tile formats and compressions
//! - Automatically detects and reads metadata files in the directory
//! - Provides asynchronous methods to fetch tile data
//!
//! ## Usage
//! ```no_run
//! use versatiles::{
//!     container::{DirectoryTilesReader, TilesReader},
//!     types::TileCoord3
//! };
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut reader = DirectoryTilesReader::open_path(Path::new("/path/to/tiles")).unwrap();
//!     let tile_data = reader.get_tile_data(&TileCoord3::new(1, 2, 3).unwrap()).await.unwrap();
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if the directory is not found, is not in the correct format, or contains inconsistent tile formats or compressions.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of opening paths, reading metadata, handling different file formats, and edge cases.

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use itertools::Itertools;
use std::{
	collections::HashMap,
	fmt::Debug,
	fs,
	path::{Path, PathBuf},
};
use versatiles_core::{
	types::{
		Blob, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TilesReader,
		TilesReaderParameters,
	},
	utils::decompress,
};

/// A reader for tiles stored in a directory structure.
/// The directory should be structured as follows:
/// ```text
/// <root>/<z>/<x>/<y>.<format>[.<compression>]
/// ```
/// Where `<z>` is the zoom level, `<x>` and `<y>` are the tile coordinates, `<format>` is the tile format, and `<compression>` is the compression type (optional).
pub struct DirectoryTilesReader {
	meta: Option<Blob>,
	dir: PathBuf,
	tile_map: HashMap<TileCoord3, PathBuf>,
	parameters: TilesReaderParameters,
}

impl DirectoryTilesReader {
	/// Opens a directory and initializes a `DirectoryTilesReader`.
	///
	/// # Arguments
	///
	/// * `dir` - A path to the directory containing the tiles.
	///
	/// # Errors
	///
	/// Returns an error if the directory does not exist, is not a directory, or if no tiles are found.
	pub fn open_path(dir: &Path) -> Result<DirectoryTilesReader>
	where
		Self: Sized,
	{
		log::trace!("read {dir:?}");

		ensure!(dir.is_absolute(), "path {dir:?} must be absolute");
		ensure!(dir.exists(), "path {dir:?} does not exist");
		ensure!(dir.is_dir(), "path {dir:?} is not a directory");

		let mut meta: Option<Blob> = None;
		let mut tile_map = HashMap::new();
		let mut container_form: Option<TileFormat> = None;
		let mut container_comp: Option<TileCompression> = None;
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

					let files = fs::read_dir(entry2.path())?.map(|f| f.unwrap());
					let files = files
						.sorted_unstable_by(|a, b| a.file_name().partial_cmp(&b.file_name()).unwrap());

					for entry3 in files {
						// y level
						let mut filename = entry3.file_name().into_string().unwrap();
						let file_comp = TileCompression::from_filename(&mut filename);
						let this_form = TileFormat::from_filename(&mut filename);

						if this_form.is_none() {
							continue;
						}
						let file_form = this_form.unwrap();

						let numeric3 = filename.parse::<u32>();
						if numeric3.is_err() {
							continue;
						}
						let y = numeric3?;

						if container_form.is_none() {
							container_form = Some(file_form);
						} else if container_form != Some(file_form) {
							bail!(
								"found multiple tile formats {:?} and {file_form:?}",
								container_form.unwrap()
							);
						}

						if container_comp.is_none() {
							container_comp = Some(file_comp);
						} else if container_comp != Some(file_comp) {
							bail!(
								"found multiple tile compressions {:?} and {file_comp:?}",
								container_comp.unwrap()
							);
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
						meta = Some(decompress(
							Self::read(&entry1.path())?,
							&TileCompression::Gzip,
						)?);
						continue;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						meta = Some(decompress(
							Self::read(&entry1.path())?,
							&TileCompression::Brotli,
						)?);
						continue;
					}
					&_ => {}
				};
			}
		}

		if tile_map.is_empty() {
			bail!("no tiles found");
		}

		let tile_format = container_form.context("tile format must be specified")?;
		let tile_compression = container_comp.context("tile compression must be specified")?;

		Ok(DirectoryTilesReader {
			meta,
			dir: dir.to_path_buf(),
			tile_map,
			parameters: TilesReaderParameters::new(tile_format, tile_compression, bbox_pyramid),
		})
	}

	fn read(path: &Path) -> Result<Blob> {
		Ok(Blob::from(fs::read(path)?))
	}
}

#[async_trait]
impl TilesReader for DirectoryTilesReader {
	fn get_container_name(&self) -> &str {
		"directory"
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
		log::trace!("get_tile_data {:?}", coord);

		if let Some(path) = self.tile_map.get(coord) {
			Self::read(path).map(Some)
		} else {
			Ok(None)
		}
	}
	fn get_name(&self) -> &str {
		self.dir.to_str().unwrap()
	}
}

impl Debug for DirectoryTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DirectoryTilesReader")
			.field("name", &self.get_name())
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::{
		fixture::{FileWriteStr, PathChild},
		TempDir,
	};
	use std::fs::{self};
	use versatiles_core::{assert_wildcard, utils::compress};

	#[tokio::test]
	async fn tile_reader_new() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child(".DS_Store").write_str("")?;
		dir.child("1/2/3.png").write_str("test tile data")?;
		dir.child("meta.json").write_str("test meta data")?;

		let mut reader = DirectoryTilesReader::open_path(&dir)?;

		assert_eq!(reader.get_meta()?.unwrap().as_str(), "test meta data");

		let tile_data = reader
			.get_tile_data(&TileCoord3::new(2, 3, 1)?)
			.await?
			.unwrap();
		assert_eq!(tile_data, Blob::from("test tile data"));

		assert!(reader
			.get_tile_data(&TileCoord3::new(2, 2, 1)?)
			.await?
			.is_none());

		Ok(())
	}

	#[tokio::test]
	async fn open_path_with_nonexistent_directory() -> Result<()> {
		let dir = TempDir::new()?;

		let msg = DirectoryTilesReader::open_path(&dir.join("dont_exist"))
			.unwrap_err()
			.to_string();
		assert_eq!(
			&msg[msg.len() - 16..],
			"\" does not exist",
			"Should return error on non-existent directory"
		);

		Ok(())
	}

	#[tokio::test]
	async fn open_path_with_unsupported_file_format() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child("1/2/3.unknown").write_str("unsupported format")?;

		assert_eq!(
			DirectoryTilesReader::open_path(dir.path())
				.unwrap_err()
				.to_string(),
			"no tiles found",
			"Should return error on unsupported file formats"
		);

		Ok(())
	}

	#[tokio::test]
	async fn read_compressed_meta_files() -> Result<()> {
		let dir = TempDir::new().unwrap();
		fs::write(
			dir.path().join("meta.json.gz"),
			compress(Blob::from("test meta data gzip"), &TileCompression::Gzip)
				.unwrap()
				.as_slice(),
		)
		.unwrap();
		fs::create_dir_all(dir.path().join("0/1")).unwrap();
		fs::write(dir.path().join("0/1/2.png"), "tile at 0/1/2").unwrap();

		let reader = DirectoryTilesReader::open_path(&dir).unwrap();
		assert_eq!(
			reader.get_meta().unwrap().unwrap(),
			Blob::from("test meta data gzip")
		);

		Ok(())
	}

	#[tokio::test]
	async fn complex_directory_structure() -> Result<()> {
		let dir = TempDir::new().unwrap();
		fs::create_dir_all(dir.path().join("0/1")).unwrap();
		fs::write(dir.path().join("0/1/2.png"), "tile at 0/1/2").unwrap();
		fs::write(dir.path().join("meta.json"), "global meta").unwrap();

		let mut reader = DirectoryTilesReader::open_path(&dir).unwrap();
		let coord = TileCoord3::new(1, 2, 0).unwrap();
		let tile_data = reader.get_tile_data(&coord).await.unwrap().unwrap();

		assert_eq!(tile_data, Blob::from("tile at 0/1/2"));

		Ok(())
	}

	#[tokio::test]
	async fn incorrect_format_and_compression_handling() -> Result<()> {
		let dir = TempDir::new().unwrap();
		fs::create_dir_all(dir.path().join("1/2")).unwrap();
		fs::write(dir.path().join("1/2/3.txt"), "wrong format").unwrap();

		assert_eq!(
			&DirectoryTilesReader::open_path(&dir)
				.unwrap_err()
				.to_string(),
			"no tiles found",
			"Should error on incorrect tile format"
		);

		Ok(())
	}

	#[tokio::test]
	async fn error_different_tile_formats() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child("1/2/3.png").write_str("test tile data")?;
		dir.child("1/2/4.jpg").write_str("test tile data")?;

		assert_eq!(
			DirectoryTilesReader::open_path(&dir)
				.unwrap_err()
				.to_string(),
			"found multiple tile formats PNG and JPG"
		);

		Ok(())
	}

	#[tokio::test]
	async fn error_different_tile_compressions() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child("1/2/3.pbf").write_str("test tile data")?;
		dir.child("1/2/4.pbf.br").write_str("test tile data")?;

		assert_eq!(
			DirectoryTilesReader::open_path(&dir)
				.unwrap_err()
				.to_string(),
			"found multiple tile compressions Uncompressed and Brotli"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_minor_functions() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		dir.child("meta.json").write_str("{\"key\": \"value\"}")?;
		dir.child("1/2/3.png.br").write_str("tile data")?;

		let mut reader = DirectoryTilesReader::open_path(dir.path())?;

		assert_eq!(reader.get_container_name(), "directory");

		assert_wildcard!(
			format!("{reader:?}"), 
			"DirectoryTilesReader { name: \"*\", parameters: TilesReaderParameters { bbox_pyramid: [1: [2,3,2,3] (1)], tile_compression: Brotli, tile_format: PNG } }"
		);

		assert_eq!(
			reader.get_meta()?.unwrap(),
			Blob::from("{\"key\": \"value\"}")
		);

		assert_eq!(
			reader.get_parameters().tile_compression,
			TileCompression::Brotli
		);
		reader.override_compression(TileCompression::Gzip);
		assert_eq!(
			reader.get_parameters().tile_compression,
			TileCompression::Gzip
		);

		Ok(())
	}
}
