//! This module provides functionality for reading tile data from a directory structure.
//!
//! The directory path must be **absolute**.
//!
//! Recognized metadata files include `meta.json`, `tiles.json`, `metadata.json` and their compressed variants with `.gz` or `.br` extensions.
//!
//! Tile files must follow the naming pattern:
//! ```text
//! <root>/<z>/<x>/<y>.<format>[.<compression>]
//! ```
//! where `<z>`, `<x>`, and `<y>` are zoom level and tile coordinates, `<format>` is the tile format (e.g., `png`, `pbf`), and `<compression>` is optional (e.g., `br`, `gz`).
//!
//! Examples:
//! | Path               | Description                  |
//! |--------------------|------------------------------|
//! | `/tiles/3/2/1.png` | Uncompressed PNG tile        |
//! | `/tiles/4/2/1.pbf.br` | Brotli compressed PBF tile  |
//! | `/tiles/meta.json`  | Metadata file                |
//!
//! All tiles must share the same **format** and **compression**. If multiple formats or compressions are detected, an error is returned.
//!
//! Bounds, minimum zoom, and maximum zoom are inferred from the discovered tiles and merged with any metadata files found.
//!
//! ## Usage
//! ```no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//! use tokio;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut reader = DirectoryReader::open_path(Path::new("/absolute/path/to/tiles")).unwrap();
//!     let tile_data = reader.get_tile(&TileCoord::new(3, 1, 2).unwrap()).await.unwrap();
//! }
//! ```
//!
//! ## Errors
//! Errors are returned if the directory is not absolute, does not exist, is not a directory, contains no tiles, or if tiles have inconsistent formats or compressions.

use crate::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use itertools::Itertools;
use std::{
	collections::HashMap,
	fmt::Debug,
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};
use versatiles_core::{
	Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, TileStream,
	compression::decompress,
};
use versatiles_derive::context;

/// A reader for tiles stored in a directory structure.
///
/// This struct merges `TileJSON` metadata from recognized files such as `meta.json`, `tiles.json`, or `metadata.json` (and their compressed variants),
/// and infers a bounding-box pyramid from the folder hierarchy to provide tile reading functionality.
///
/// The directory structure is expected as:
/// ```text
/// <root>/<z>/<x>/<y>.<format>[.<compression>]]
/// ```
/// where `<z>`, `<x>`, and `<y>` are tile coordinates, `<format>` is the tile format, and `<compression>` is optional.
pub struct DirectoryReader {
	tilejson: TileJSON,
	dir: PathBuf,
	tile_map: Arc<HashMap<TileCoord, PathBuf>>,
	metadata: TileSourceMetadata,
}

impl DirectoryReader {
	/// Opens a directory and initializes a `DirectoryReader`.
	///
	/// The provided path must be **absolute**.
	///
	/// This function scans the directory structure for tiles and metadata files.
	/// It requires that all tiles have a uniform tile format and compression type, otherwise it returns an error.
	/// Metadata files (`meta.json`, `tiles.json`, `metadata.json` and their `.gz`/`.br` variants) are merged into the `TileJSON`.
	/// Bounds, minzoom, and maxzoom are inferred from the directory's tile pyramid and merged with metadata.
	///
	/// The returned `DirectoryReader` contains `TileSourceMetadata` which specify the tile format, compression, and bounding box pyramid.
	///
	/// # Arguments
	///
	/// * `dir` - An absolute path to the directory containing the tiles.
	///
	/// # Errors
	///
	/// Returns an error if the directory does not exist, is not a directory, contains no tiles, or contains inconsistent tile formats or compressions.
	#[context("opening tiles directory {:?}", dir)]
	pub fn open_path(dir: &Path) -> Result<DirectoryReader>
	where
		Self: Sized,
	{
		log::trace!("read {dir:?}");

		ensure!(dir.is_absolute(), "path {dir:?} must be absolute");
		ensure!(dir.exists(), "path {dir:?} does not exist");
		ensure!(dir.is_dir(), "path {dir:?} is not a directory");

		let mut tilejson = TileJSON::default();
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
				let level = numeric1?;

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
					let files = files.sorted_unstable_by(|a, b| a.file_name().partial_cmp(&b.file_name()).unwrap());

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

						if let Some(form) = container_form {
							if form != file_form {
								let mut r = [form, file_form];
								r.sort();
								bail!("found multiple tile formats: {r:?}");
							}
						} else {
							container_form = Some(file_form);
						}

						if let Some(comp) = container_comp {
							if comp != file_comp {
								let mut r = [comp, file_comp];
								r.sort();
								bail!("found multiple tile compressions: {r:?}");
							}
						} else {
							container_comp = Some(file_comp);
						}

						let coord = TileCoord::new(level, x, y)?;
						bbox_pyramid.include_coord(&coord);
						tile_map.insert(coord, entry3.path());
					}
				}
			} else {
				match name1.as_str() {
					"meta.json" | "tiles.json" | "metadata.json" => {
						tilejson.merge(&TileJSON::try_from_blob_or_default(&Self::read(&entry1.path())?))?;
					}
					"meta.json.gz" | "tiles.json.gz" | "metadata.json.gz" => {
						tilejson.merge(&TileJSON::try_from_blob_or_default(&decompress(
							Self::read(&entry1.path())?,
							TileCompression::Gzip,
						)?))?;
					}
					"meta.json.br" | "tiles.json.br" | "metadata.json.br" => {
						tilejson.merge(&TileJSON::try_from_blob_or_default(&decompress(
							Self::read(&entry1.path())?,
							TileCompression::Brotli,
						)?))?;
					}
					&_ => {}
				}
			}
		}

		if tile_map.is_empty() {
			bail!("no tiles found");
		}

		let tile_format = container_form.context("tile format must be specified")?;
		let tile_compression = container_comp.context("tile compression must be specified")?;

		tilejson.update_from_pyramid(&bbox_pyramid);

		Ok(DirectoryReader {
			tilejson,
			dir: dir.to_path_buf(),
			tile_map: Arc::new(tile_map),
			metadata: TileSourceMetadata::new(tile_format, tile_compression, bbox_pyramid, Traversal::ANY),
		})
	}

	/// Reads a file into a `Blob`.
	#[context("reading file '{}'", path.display())]
	fn read(path: &Path) -> Result<Blob> {
		Ok(Blob::from(fs::read(path)?))
	}

	/// Internal helper to look up and read a tile by coordinate.
	fn lookup_tile(
		coord: &TileCoord,
		tile_map: &HashMap<TileCoord, PathBuf>,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> Result<Option<Tile>> {
		if let Some(path) = tile_map.get(coord) {
			Self::read(path).map(|blob| Some(Tile::from_blob(blob, tile_compression, tile_format)))
		} else {
			Ok(None)
		}
	}
}

/// Implements the `TileSource` for `DirectoryReader`.
///
/// Provides the container name ("directory"), access to tile reading parameters,
/// ability to override the tile compression, access to `TileJSON` metadata,
/// and asynchronous fetching of tile data by coordinate.
#[async_trait]
impl TileSource for DirectoryReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("directory", self.dir.to_str().unwrap())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	#[context("fetching tile {:?} from directory '{}'", coord, self.dir.display())]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		log::trace!("get_tile {coord:?}");
		Self::lookup_tile(
			coord,
			&self.tile_map,
			self.metadata.tile_compression,
			self.metadata.tile_format,
		)
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let tile_map = Arc::clone(&self.tile_map);
		let tile_compression = self.metadata.tile_compression;
		let tile_format = self.metadata.tile_format;

		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			DirectoryReader::lookup_tile(&coord, &tile_map, tile_compression, tile_format).ok()?
		}))
	}
}

impl Debug for DirectoryReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DirectoryReader")
			.field("source_type", &self.source_type())
			.field("parameters", &self.metadata())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::{
		TempDir,
		fixture::{FileWriteStr, PathChild},
	};
	use std::fs::{self};
	use versatiles_core::{assert_wildcard, compression::compress};

	#[tokio::test]
	async fn tile_reader_new() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child(".DS_Store").write_str("")?;
		dir.child("3/2/1.png").write_str("test tile data")?;
		dir.child("meta.json").write_str(r#"{"type":"dummy"}"#)?;

		let reader = DirectoryReader::open_path(&dir)?;

		assert_eq!(
			reader.tilejson().as_string(),
			"{\"bounds\":[-90,66.51326,-45,79.171335],\"center\":[-67.5,72.842298,3],\"maxzoom\":3,\"minzoom\":3,\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);

		let mut tile_data = reader.get_tile(&TileCoord::new(3, 2, 1)?).await?.unwrap();
		assert_eq!(
			tile_data.as_blob(reader.metadata().tile_compression)?,
			&Blob::from("test tile data")
		);

		assert!(reader.get_tile(&TileCoord::new(2, 2, 1)?).await?.is_none());

		Ok(())
	}

	#[tokio::test]
	async fn open_path_with_nonexistent_directory() -> Result<()> {
		let dir = TempDir::new()?;

		let msg = DirectoryReader::open_path(&dir.join("dont_exist"))
			.unwrap_err()
			.chain()
			.last()
			.unwrap()
			.to_string();
		assert_eq!(&msg[msg.len() - 16..], "\" does not exist");

		Ok(())
	}

	#[tokio::test]
	async fn open_path_with_unsupported_file_format() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child("3/2/1.unknown").write_str("unsupported format")?;

		assert_eq!(
			DirectoryReader::open_path(dir.path())
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"no tiles found",
		);

		Ok(())
	}

	#[tokio::test]
	async fn read_compressed_meta_files() -> Result<()> {
		let dir = TempDir::new().unwrap();
		fs::write(
			dir.path().join("meta.json.gz"),
			compress(Blob::from(r#"{"type":"dummy data"}"#), TileCompression::Gzip)
				.unwrap()
				.as_slice(),
		)
		.unwrap();
		fs::create_dir_all(dir.path().join("2/1")).unwrap();
		fs::write(dir.path().join("2/1/0.png"), "tile at 2/1/0").unwrap();

		let reader = DirectoryReader::open_path(&dir).unwrap();
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"bounds\":[-90,66.51326,0,85.051129],\"center\":[-45,75.782195,2],\"maxzoom\":2,\"minzoom\":2,\"tilejson\":\"3.0.0\",\"type\":\"dummy data\"}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn complex_directory_structure() -> Result<()> {
		let dir = TempDir::new().unwrap();
		fs::create_dir_all(dir.path().join("3/2")).unwrap();
		fs::write(dir.path().join("3/2/1.png"), "tile at 3/2/1").unwrap();
		fs::write(dir.path().join("meta.json"), r#"{"type":"dummy data"}"#).unwrap();

		let reader = DirectoryReader::open_path(&dir).unwrap();
		let coord = TileCoord::new(3, 2, 1).unwrap();
		let blob = reader
			.get_tile(&coord)
			.await
			.unwrap()
			.unwrap()
			.into_blob(reader.metadata().tile_compression)?;

		assert_eq!(blob, Blob::from("tile at 3/2/1"));

		Ok(())
	}

	#[tokio::test]
	async fn incorrect_format_and_compression_handling() -> Result<()> {
		let dir = TempDir::new().unwrap();
		fs::create_dir_all(dir.path().join("3/2")).unwrap();
		fs::write(dir.path().join("3/2/1.txt"), "wrong format").unwrap();

		assert_eq!(
			&DirectoryReader::open_path(&dir)
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"no tiles found",
			"Should error on incorrect tile format"
		);

		Ok(())
	}

	#[tokio::test]
	async fn error_different_tile_formats() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child("3/2/1.png").write_str("test tile data")?;
		dir.child("4/2/1.jpg").write_str("test tile data")?;

		assert_eq!(
			DirectoryReader::open_path(&dir)
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"found multiple tile formats: [JPG, PNG]"
		);

		Ok(())
	}

	#[tokio::test]
	async fn error_different_tile_compressions() -> Result<()> {
		let dir = TempDir::new()?;
		dir.child("3/2/1.pbf").write_str("test tile data")?;
		dir.child("4/2/1.pbf.br").write_str("test tile data")?;

		assert_eq!(
			DirectoryReader::open_path(&dir)
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"found multiple tile compressions: [Uncompressed, Brotli]"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_minor_functions() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		dir.child("meta.json").write_str("{\"key\": \"value\"}")?;
		dir.child("3/2/1.png.br").write_str("tile data")?;

		let reader = DirectoryReader::open_path(dir.path())?;

		assert_eq!(
			reader.source_type().to_string(),
			format!("container 'directory' ('{}')", dir.path().to_str().unwrap())
		);

		assert_wildcard!(
			format!("{reader:?}"),
			"DirectoryReader { source_type: Container { name: \"directory\", uri: \"*\" }, parameters: TileSourceMetadata { bbox_pyramid: [3: [2,1,2,1] (1x1)], tile_compression: Brotli, tile_format: PNG, traversal: Traversal(AnyOrder,full) } }"
		);

		assert_eq!(
			reader.tilejson().as_string(),
			"{\"bounds\":[-90,66.51326,-45,79.171335],\"center\":[-67.5,72.842298,3],\"key\":\"value\",\"maxzoom\":3,\"minzoom\":3,\"tilejson\":\"3.0.0\"}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_matches_individual_reads() -> Result<()> {
		let dir = TempDir::new()?;
		// Create a small grid of tiles
		dir.child("2/0/0.png").write_str("tile_0_0")?;
		dir.child("2/0/1.png").write_str("tile_0_1")?;
		dir.child("2/1/0.png").write_str("tile_1_0")?;
		dir.child("2/1/1.png").write_str("tile_1_1")?;

		let reader = DirectoryReader::open_path(&dir)?;
		let bbox = TileBBox::from_min_and_max(2, 0, 0, 1, 1)?;

		// Get all tiles via stream
		let stream = reader.get_tile_stream(bbox).await?;
		let stream_tiles: Vec<_> = stream.to_vec().await;
		assert_eq!(stream_tiles.len(), 4);

		// Verify each streamed tile matches individual read
		for (coord, mut tile) in stream_tiles {
			let stream_blob = tile.as_blob(reader.metadata().tile_compression)?;
			let single_blob = reader
				.get_tile(&coord)
				.await?
				.expect("tile should exist")
				.into_blob(reader.metadata().tile_compression)?;
			assert_eq!(
				stream_blob.as_slice(),
				single_blob.as_slice(),
				"blob mismatch at {coord:?}"
			);
		}

		Ok(())
	}
}
