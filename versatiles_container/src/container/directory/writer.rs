//! This module provides functionality for writing tile data to a directory structure.
//!
//! The `DirectoryTilesWriter` struct is the primary component of this module, offering methods to write metadata and tile data to a specified directory path.
//!
//! ## Directory Structure
//! The directory structure for writing tiles follows the same format as reading:
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
//! - Supports writing metadata and tile data in multiple formats and compressions
//! - Ensures directory structure is created if it does not exist
//! - Provides progress feedback during the write process
//!
//! ## Usage
//! ```rust
//! use versatiles::container::{DirectoryTilesWriter, MBTilesReader, TilesWriterTrait};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() {
//!     let path = std::env::current_dir().unwrap().join("../testdata/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(&path).unwrap();
//!
//!     let temp_path = std::env::temp_dir().join("temp_tiles");
//!     DirectoryTilesWriter::write_to_path(&mut reader, &temp_path).await.unwrap();
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if the directory path is not absolute, if there are issues with file I/O, or if multiple tile formats/compressions are found.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of writing metadata, handling different file formats, and verifying directory structure.

use crate::container::TilesWriterTrait;
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use std::{
	fs,
	path::{Path, PathBuf},
};
use versatiles_core::{
	types::{Blob, TilesReaderTrait},
	utils::{compress, io::DataWriterTrait, progress::get_progress_bar},
};

/// A struct that provides functionality to write tile data to a directory structure.
pub struct DirectoryTilesWriter {}

impl DirectoryTilesWriter {
	/// Writes the given blob to the specified path. Creates the necessary directory structure if it doesn't exist.
	///
	/// # Arguments
	/// * `path` - The path where the blob should be written.
	/// * `blob` - The blob data to write.
	///
	/// # Errors
	/// Returns an error if the parent directory cannot be created or if writing to the file fails.
	fn write(path: PathBuf, blob: Blob) -> Result<()> {
		let parent = path.parent().unwrap();
		if !parent.exists() {
			fs::create_dir_all(parent)?;
		}

		fs::write(&path, blob.as_slice())?;
		Ok(())
	}
}

#[async_trait]
impl TilesWriterTrait for DirectoryTilesWriter {
	/// Writes the tile data and metadata from the given `TilesReader` to the specified directory path.
	///
	/// # Arguments
	/// * `reader` - A mutable reference to the `TilesReader` providing the data.
	/// * `path` - The directory path where the data should be written.
	///
	/// # Errors
	/// Returns an error if the path is not absolute, if there are issues with file I/O, or if compression fails.
	async fn write_to_path(reader: &mut dyn TilesReaderTrait, path: &Path) -> Result<()> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		log::trace!("convert_from");

		let parameters = reader.get_parameters();
		let tile_compression = &parameters.tile_compression.clone();
		let tile_format = &parameters.tile_format.clone();
		let bbox_pyramid = &reader.get_parameters().bbox_pyramid.clone();

		let extension_format = tile_format.extension();
		let extension_compression = tile_compression.extension();

		let meta_data_option = reader.get_meta()?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data.into(), tile_compression)?;
			let filename = format!("tiles.json{extension_compression}");

			Self::write(path.join(filename), meta_data)?;
		}

		let mut progress = get_progress_bar("converting tiles", bbox_pyramid.count_tiles());

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox.clone()).await;

			while let Some(entry) = stream.next().await {
				let (coord, blob) = entry;

				progress.inc(1);

				let filename = format!(
					"{}/{}/{}{}{}",
					coord.z, coord.y, coord.x, extension_format, extension_compression
				);

				// Write blob to file
				Self::write(path.join(filename), blob)?;
			}
		}

		progress.finish();

		Ok(())
	}

	/// Writes the tile data from the given `TilesReader` to the specified `DataWriterTrait`.
	///
	/// # Arguments
	/// * `reader` - A mutable reference to the `TilesReader` providing the data.
	/// * `writer` - A mutable reference to the `DataWriterTrait` where the data should be written.
	///
	/// # Errors
	/// This function always returns an error as it is not implemented.
	async fn write_to_writer(
		_reader: &mut dyn TilesReaderTrait,
		_writer: &mut dyn DataWriterTrait,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::{MockTilesReader, MOCK_BYTES_PBF};
	use versatiles_core::{types::*, utils::decompress_gzip};

	/// Tests the functionality of writing tile data to a directory from a mock reader.
	#[tokio::test]
	async fn test_convert_from() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let temp_path = temp_dir.path();

		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters::new(
			TileFormat::PBF,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full(2),
		))?;

		DirectoryTilesWriter::write_to_path(&mut mock_reader, temp_path).await?;

		let load = |filename| {
			let path = temp_path.join(filename);
			path
				.try_exists()
				.unwrap_or_else(|_| panic!("filename {filename} should exist"));
			decompress_gzip(&Blob::from(
				fs::read(path).unwrap_or_else(|_| panic!("filename {filename} should be readable")),
			))
			.unwrap_or_else(|_| panic!("filename {filename} should be gzip compressed"))
		};

		assert_eq!(load("tiles.json.gz").as_str(), "{\"type\":\"dummy\"}");
		assert_eq!(load("0/0/0.pbf.gz").as_slice(), MOCK_BYTES_PBF);
		assert_eq!(load("2/3/3.pbf.gz").as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}
}
