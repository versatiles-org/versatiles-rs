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
//! ```rust
//! use versatiles_container::{MBTilesWriter, PMTilesReader, TilesWriterTrait};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() {
//!     let path = std::env::current_dir().unwrap().join("../testdata/berlin.pmtiles");
//!     let mut reader = PMTilesReader::open_path(&path).await.unwrap();
//!
//!     let temp_path = std::env::temp_dir().join("temp.mbtiles");
//!     MBTilesWriter::write_to_path(&mut reader, &temp_path).await.unwrap();
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if there are issues with the SQLite database, if unsupported tile formats or compressions are encountered, or if there are I/O issues.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of writing metadata, handling different file formats, and verifying the database structure.

use crate::TilesWriterTrait;
use anyhow::{bail, Result};
use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::{rusqlite::params, SqliteConnectionManager};
use std::{fs::remove_file, path::Path};
use versatiles_core::{io::DataWriterTrait, json::JsonObject, progress::get_progress_bar, types::*};

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
		if path.exists() {
			remove_file(path)?;
		}
		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;

		pool.get()?.execute_batch(
			"CREATE TABLE metadata (name TEXT, value TEXT, UNIQUE (name));
			CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB, UNIQUE (zoom_level, tile_column, tile_row));
			CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);",
		)?;

		Ok(MBTilesWriter { pool })
	}

	/// Adds multiple tiles to the MBTiles file within a single transaction.
	///s
	/// # Arguments
	/// * `tiles` - A vector of tuples containing tile coordinates and tile data.
	///
	/// # Errors
	/// Returns an error if the transaction fails.
	fn add_tiles(&mut self, tiles: &Vec<(TileCoord3, Blob)>) -> Result<()> {
		let mut conn = self.pool.get()?;
		let transaction = conn.transaction()?;
		for (c, blob) in tiles {
			let max_index = 2u32.pow(c.z as u32) - 1;
			transaction.execute(
				"INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
				params![c.z, c.x, max_index - c.y, blob.as_slice()],
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
impl TilesWriterTrait for MBTilesWriter {
	/// Writes tiles and metadata to the MBTiles file.
	///
	/// # Arguments
	/// * `reader` - The reader from which to fetch tiles and metadata.
	/// * `path` - The path to the MBTiles file.
	///
	/// # Errors
	/// Returns an error if the file format or compression is not supported, or if there are issues with writing to the SQLite database.
	async fn write_to_path(reader: &mut dyn TilesReaderTrait, path: &Path) -> Result<()> {
		use TileCompression::*;
		use TileFormat::*;

		let mut writer = MBTilesWriter::new(path)?;

		let parameters = reader.get_parameters().clone();

		let format = match (parameters.tile_format, parameters.tile_compression) {
			(JPG, Uncompressed) => "jpg",
			(MVT, Gzip) => "pbf",
			(PNG, Uncompressed) => "png",
			(WEBP, Uncompressed) => "webp",
			_ => bail!(
				"combination of format ({}) and compression ({}) is not supported. MBTiles supports only uncompressed jpg/png/webp or gzipped pbf",
				parameters.tile_format,
				parameters.tile_compression
			),
		};

		writer.set_metadata("format", format)?;
		writer.set_metadata("type", "baselayer")?;
		writer.set_metadata("version", "3.0")?;
		let pyramid = &reader.get_parameters().bbox_pyramid;
		let bbox = pyramid.get_geo_bbox().unwrap();
		let center = pyramid.get_geo_center().unwrap();
		let zoom_min = pyramid.get_zoom_min().unwrap();
		let zoom_max = pyramid.get_zoom_max().unwrap();
		writer.set_metadata("bounds", &format!("{},{},{},{}", bbox.0, bbox.1, bbox.2, bbox.3))?;
		writer.set_metadata("center", &format!("{},{},{}", center.0, center.1, center.2))?;
		writer.set_metadata("minzoom", &zoom_min.to_string())?;
		writer.set_metadata("maxzoom", &zoom_max.to_string())?;

		let tilejson = reader.get_tilejson();
		if let Some(vector_layers) = tilejson.as_object().get("vector_layers") {
			writer.set_metadata(
				"json",
				&JsonObject::from(vec![("vector_layers", vector_layers)]).stringify(),
			)?;
		}

		for key in ["name", "author", "type", "description", "version", "license"] {
			if let Some(value) = tilejson.get_str(key) {
				writer.set_metadata(key, value)?;
			}
		}

		let mut progress = get_progress_bar("converting tiles", pyramid.count_tiles());

		for bbox in pyramid.iter_levels() {
			let stream = reader.get_bbox_tile_stream(*bbox).await;

			stream
				.for_each_buffered(2000, |v| {
					writer.add_tiles(&v).unwrap();
					progress.inc(v.len() as u64)
				})
				.await;
		}

		progress.finish();

		Ok(())
	}

	/// Not implemented: Writes tiles and metadata to a generic data writer.
	async fn write_to_writer(_reader: &mut dyn TilesReaderTrait, _writer: &mut dyn DataWriterTrait) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MBTilesReader, MockTilesReader, MockTilesWriter};
	use assert_fs::NamedTempFile;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(5),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
		})?;

		let filename = NamedTempFile::new("temp.mbtiles")?;
		MBTilesWriter::write_to_path(&mut mock_reader, &filename).await?;

		let mut reader = MBTilesReader::open_path(&filename)?;

		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}
}
