//! Write tile data and metadata to a directory pyramid on disk.
//!
//! The writer mirrors the on-disk structure used by the directory reader:
//!
//! ```text
//! <root>/<z>/<x>/<y>.<format>[.<compression>]
//! ```
//!
//! where `<format>` is the tile format (e.g., `png`, `pbf`/`mvt`) and `<compression>` is optional (`br`, `gz`).
//! A TileJSON file is written as `tiles.json[.<compression>]` using the same **compression** as the tiles.
//!
//! ### Requirements
//! - The output `path` **must be absolute**.
//! - All emitted tiles use the **same format** and **compression** as reported by the source reader's
//!   [`TileSourceMetadata`](versatiles_core::TileSourceMetadata).
//! - The directory tree is created as needed.
//!
//! ### Recognized outputs
//! - Tiles at `<z>/<x>/<y>.<ext>[.<br|gz>]` (e.g., `2/3/1.pbf.gz`, `7/21/42.png`).
//! - TileJSON at `tiles.json[.<br|gz>]`.
//!
//! ### Example
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let runtime = TilesRuntime::default();
//!
//!     // Open any reader, e.g. MBTiles
//!     let mbtiles_path = Path::new("/absolute/path/to/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(mbtiles_path, runtime.clone())?;
//!
//!     // Choose an absolute output directory
//!     let out_dir = std::env::temp_dir().join("versatiles_demo_out");
//!     DirectoryTilesWriter::write_to_path(&mut reader, &out_dir, runtime).await?;
//!     Ok(())
//! }
//! ```
//!
//! ### Errors
//! Returns errors if the destination path is not absolute, if file I/O fails, or if compression/encoding fails.

use crate::{TileSourceTrait, TilesReaderTraverseExt, TilesRuntime, TilesWriterTrait, Traversal};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use std::{
	fs,
	path::{Path, PathBuf},
};
use versatiles_core::{io::DataWriterTrait, utils::compress, *};
use versatiles_derive::context;

/// Writes a directory-based tile pyramid along with a compressed TileJSON (`tiles.json[.<br|gz>]`).
///
/// Tiles are encoded using the format and compression from the source `TilesReader`. The
/// writer creates intermediate directories on demand and preserves the `{z}/{x}/{y}` layout.
pub struct DirectoryTilesWriter {}

impl DirectoryTilesWriter {
	/// Write a `Blob` to `path`, creating missing parent directories.
	#[context("writing file '{}'", path.display())]
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
	/// Write all tiles and metadata from `reader` into the absolute directory `path`.
	///
	/// * Validates that `path` is absolute.
	/// * Encodes tiles using `reader.parameters().tile_format` and `reader.parameters().tile_compression`.
	/// * Writes `tiles.json[.<compression>]` containing the reader's TileJSON, compressed to the same transport layer.
	/// * Creates the `{z}/{x}/{y}` directory structure on demand.
	///
	/// # Errors
	/// Returns an error for non-absolute paths, I/O failures, or encoding/compression errors.
	#[context("writing tiles to directory '{}'", path.display())]
	async fn write_to_path(reader: &mut dyn TileSourceTrait, path: &Path, runtime: TilesRuntime) -> Result<()> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		log::trace!("convert_from");

		let parameters = reader.parameters();
		let tile_format = parameters.tile_format;

		let extension_format = tile_format.as_extension().to_string();
		let tile_compression = reader.parameters().tile_compression;
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
							Self::write(path.join(filename), tile.into_blob(tile_compression)?)?;
						}
						Ok(())
					})
				},
				runtime.clone(),
				None,
			)
			.await?;

		Ok(())
	}

	/// Writes the tile data from the given `TilesReader` to the specified `DataWriterTrait`.
	///
	/// # Errors
	/// Always returns an error (`not implemented`).
	#[context("writing tiles to external writer")]
	async fn write_to_writer(
		_reader: &mut dyn TileSourceTrait,
		_writer: &mut dyn DataWriterTrait,
		_runtime: TilesRuntime,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MOCK_BYTES_PBF, MockTilesReader, TileSourceMetadata};
	use versatiles_core::utils::decompress_gzip;

	/// Tests the functionality of writing tile data to a directory from a mock reader.
	#[tokio::test]
	async fn test_convert_from() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let temp_path = temp_dir.path();

		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata::new(
			TileFormat::MVT,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full(2),
		))?;

		DirectoryTilesWriter::write_to_path(&mut mock_reader, temp_path, TilesRuntime::default()).await?;

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
