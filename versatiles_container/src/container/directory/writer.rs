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
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() {
//!     let path = std::env::current_dir().unwrap().join("../testdata/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(&path).unwrap();
//!
//!     let temp_path = std::env::temp_dir().join("temp_tiles");
//!     DirectoryTilesWriter::write_to_path(
//!         &mut reader,
//!         &temp_path,
//!         TileCompression::Uncompressed,
//!         Config::default().arc()
//!     ).await.unwrap();
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if the directory path is not absolute, if there are issues with file I/O, or if multiple tile formats/compressions are found.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of writing metadata, handling different file formats, and verifying directory structure.

use crate::{Config, TilesReaderTrait, TilesReaderTraverseExt, TilesWriterTrait};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use std::{
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};
use versatiles_core::{io::DataWriterTrait, utils::compress, *};

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
	async fn write_to_path(
		reader: &mut dyn TilesReaderTrait,
		path: &Path,
		tile_compression: TileCompression,
		config: Arc<Config>,
	) -> Result<()> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		log::trace!("convert_from");

		let parameters = reader.parameters();
		let tile_format = parameters.tile_format;

		let extension_format = tile_format.as_extension().to_string();
		let extension_compression = tile_compression.as_extension().to_string();

		let tilejson = reader.tilejson();
		let meta_data = compress(tilejson.into(), tile_compression)?;
		let filename = format!("tiles.json{extension_compression}");
		Self::write(path.join(filename), meta_data)?;

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				move |_bbox, mut stream| {
					let extension_format = extension_format.clone();
					let extension_compression = extension_compression.clone();
					let path = path.to_path_buf();
					Box::pin(async move {
						while let Some(entry) = stream.next().await {
							let (coord, tile) = entry;

							let filename = format!(
								"{}/{}/{}{}{}",
								coord.level, coord.x, coord.y, extension_format, extension_compression
							);

							// Write blob to file
							Self::write(path.join(filename), tile.into_blob(tile_compression))?;
						}
						Ok(())
					})
				},
				config,
			)
			.await?;

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
		_compression: TileCompression,
		_config: Arc<Config>,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MOCK_BYTES_PBF, MockTilesReader};
	use versatiles_core::utils::decompress_gzip;

	/// Tests the functionality of writing tile data to a directory from a mock reader.
	#[tokio::test]
	async fn test_convert_from() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let temp_path = temp_dir.path();

		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters::new(
			TileFormat::MVT,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full(2),
		))?;

		DirectoryTilesWriter::write_to_path(
			&mut mock_reader,
			temp_path,
			TileCompression::Gzip,
			Config::default().arc(),
		)
		.await?;

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

		assert_eq!(
			load("tiles.json.gz").as_str(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		assert_eq!(load("0/0/0.pbf.gz").as_slice(), MOCK_BYTES_PBF);
		assert_eq!(load("2/3/3.pbf.gz").as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}
}
