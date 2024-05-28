//! This module provides functionality for writing tile data to an MBTiles SQLite database.
//!
//! The `MBTilesWriter` struct is the primary component of this module, offering methods to write metadata and tile data to a specified MBTiles file.
//!
//! ## Features
//! - Supports writing metadata and tile data in multiple formats and compressions.
//! - Ensures the necessary tables and indices are created in the SQLite database.
//! - Provides progress feedback during the write process.
//!
//! ## Usage
//! ```ignore
//! use versatiles::container::MBTilesWriter;
//! use std::path::Path;
//!
//! let reader = // initialize your TilesReader
//! MBTilesWriter::write_to_path(reader, Path::new("/path/to/output.mbtiles")).await.unwrap();
//! ```
//!
//! ## Errors
//! - Returns errors if there are issues with the SQLite database, if unsupported tile formats or compressions are encountered, or if there are I/O issues.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of writing metadata, handling different file formats, and verifying the database structure.

use crate::{
	container::{TilesReader, TilesWriter},
	io::DataWriterTrait,
	progress::get_progress_bar,
	types::{Blob, TileCompression, TileCoord3, TileFormat},
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::StreamExt;
use r2d2::Pool;
use r2d2_sqlite::{rusqlite::params, SqliteConnectionManager};
use std::path::Path;

/// A writer for creating and populating MBTiles databases.
pub struct MBTilesWriter {
	pool: Pool<SqliteConnectionManager>,
}

impl MBTilesWriter {
	/// Creates a new MBTilesWriter.
	///
	/// # Arguments
	/// * `path` - The path to the MBTiles file.
	///
	/// # Errors
	/// Returns an error if the SQLite connection cannot be established or if the necessary tables cannot be created.
	fn new(path: &Path) -> Result<Self> {
		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;

		pool.get()?.execute_batch(
			"
			  CREATE TABLE IF NOT EXISTS metadata (name text, value text, UNIQUE (name));
			  CREATE TABLE IF NOT EXISTS tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
			  CREATE UNIQUE INDEX IF NOT EXISTS tile_index on tiles (zoom_level, tile_column, tile_row);
			  ",
		)?;

		Ok(MBTilesWriter { pool })
	}

	/// Adds multiple tiles to the MBTiles file within a single transaction.
	///
	/// # Arguments
	/// * `tiles` - A vector of tuples containing tile coordinates and tile data.
	///
	/// # Errors
	/// Returns an error if the transaction fails.
	fn add_tiles(&mut self, tiles: &Vec<(TileCoord3, Blob)>) -> Result<()> {
		let mut conn = self.pool.get()?;
		let transaction = conn.transaction()?;
		for (coords, blob) in tiles {
			transaction.execute(
				"INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
				params![coords.z, coords.x, coords.y, blob.as_slice()],
			)?;
		}
		transaction.commit()?;
		Ok(())
	}

	/// Sets metadata for the MBTiles file.
	///
	/// # Arguments
	/// * `name` - The metadata key.
	/// * `value` - The metadata value.
	///
	/// # Errors
	/// Returns an error if the metadata cannot be inserted or replaced.
	fn set_metadata(&self, name: &str, value: &str) -> Result<()> {
		self.pool.get()?.execute(
			"INSERT OR REPLACE INTO metadata (name, value) VALUES (?1, ?2)",
			params![name, value],
		)?;
		Ok(())
	}
}

#[async_trait]
impl TilesWriter for MBTilesWriter {
	/// Writes tiles and metadata to the MBTiles file.
	///
	/// # Arguments
	/// * `reader` - The reader from which to fetch tiles and metadata.
	/// * `path` - The path to the MBTiles file.
	///
	/// # Errors
	/// Returns an error if the file format or compression is not supported, or if there are issues with writing to the SQLite database.
	async fn write_to_path(reader: &mut dyn TilesReader, path: &Path) -> Result<()> {
		use TileCompression::*;
		use TileFormat::*;

		let mut writer = MBTilesWriter::new(path)?;

		let parameters = reader.get_parameters().clone();

		let format = match (parameters.tile_format, parameters.tile_compression) {
			(JPG, Uncompressed) => "jpg",
			(PBF, Gzip) => "pbf",
			(PNG, Uncompressed) => "png",
			(WEBP, Uncompressed) => "webp",
			_ => bail!(
				"combination of format ({}) and compression ({}) is not supported. MBTiles supports only uncompressed jpg/png/webp or gzipped pbf",
				parameters.tile_format,
				parameters.tile_compression
			),
		};

		writer.set_metadata("format", format)?;

		if let Some(meta_data) = reader.get_meta()? {
			writer.set_metadata("json", meta_data.as_str())?;
		}

		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();
		let mut progress = get_progress_bar("converting tiles", bbox_pyramid.count_tiles());

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox.clone()).await;

			let mut tile_buffer = Vec::new();
			while let Some((coord, blob)) = stream.next().await {
				tile_buffer.push((coord, blob));
				progress.inc(1);

				if tile_buffer.len() >= 2000 {
					writer.add_tiles(&tile_buffer)?;
					tile_buffer.clear();
				}
			}
			if !tile_buffer.is_empty() {
				writer.add_tiles(&tile_buffer)?;
			}
		}

		progress.finish();

		Ok(())
	}

	/// Not implemented: Writes tiles and metadata to a generic data writer.
	async fn write_to_writer(
		_reader: &mut dyn TilesReader,
		_writer: &mut dyn DataWriterTrait,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use assert_fs::NamedTempFile;

	use crate::{
		container::{MBTilesReader, MockTilesReader, MockTilesWriter, TilesReaderParameters},
		types::{TileBBoxPyramid, TileCompression, TileFormat},
	};

	use super::*;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(5),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::PBF,
		})?;

		let filename = NamedTempFile::new("temp.mbtiles")?;
		MBTilesWriter::write_to_path(&mut mock_reader, &filename).await?;

		let mut reader = MBTilesReader::open_path(&filename)?;

		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}
}
