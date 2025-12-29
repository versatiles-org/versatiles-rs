//! Write tiles and metadata into an MBTiles (SQLite) database.
//!
//! The `MBTilesWriter` builds an MBTiles container compatible with the
//! [Mapbox MBTiles 1.3 specification](https://github.com/mapbox/mbtiles-spec).
//! It writes both the `tiles` and `metadata` tables, ensuring proper
//! coordinate flipping (XYZ → TMS), and creates required indices.
//!
//! ## Supported formats
//! - Raster tiles: `png`, `jpg`, `webp` (uncompressed)
//! - Vector tiles: `pbf` (gzipped MVT)
//!
//! ## Directory structure and schema
//! The database schema consists of:
//! - `metadata` — key-value pairs describing the dataset
//! - `tiles` — containing columns `(zoom_level, tile_column, tile_row, tile_data)`
//!
//! Coordinates are stored in the **TMS layout**, meaning that the Y coordinate
//! is flipped from the XYZ input (`tile_row = 2^z - 1 - y`).
//!
//! ## Requirements
//! - The destination path **must be absolute** and writable.
//! - The combination of format and compression must match the supported table above.
//! - All tiles must share the same format and compression.
//!
//! ## Example
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Read any existing source (e.g. PMTiles, Directory)
//!     let runtime = TilesRuntime::default();
//!     let source_path = Path::new("/absolute/path/to/berlin.pmtiles");
//!     let mut reader = PMTilesReader::open_path(&source_path, runtime.clone()).await?;
//!
//!     // Write to an MBTiles file
//!     let out_file = std::env::temp_dir().join("berlin.mbtiles");
//!     MBTilesWriter::write_to_path(&mut reader, &out_file, runtime).await?;
//!     Ok(())
//! }
//! ```

use crate::{TileSource, TileSourceTraverseExt, TilesRuntime, TilesWriter, Traversal};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::lock::Mutex;
use r2d2::Pool;
use r2d2_sqlite::{SqliteConnectionManager, rusqlite::params};
use std::{fs::remove_file, path::Path, sync::Arc};
use versatiles_core::{io::DataWriterTrait, json::JsonObject, *};
use versatiles_derive::context;

/// Writer for MBTiles (SQLite) containers.
///
/// Creates a new SQLite database, initializes the `tiles` and `metadata` tables,
/// and writes all tile blobs and associated metadata from a `TilesReader`.
/// Each tile is stored as one record with XYZ coordinates flipped to TMS indexing.
///
/// This writer ensures MBTiles compatibility and writes a minimal, valid dataset
/// ready for use in tools such as MapLibre, Mapbox GL, or GDAL.
pub struct MBTilesWriter {
	pool: Pool<SqliteConnectionManager>,
}

impl MBTilesWriter {
	/// Create a new MBTiles writer at the specified path.
	///
	/// If a file already exists, it is removed. The method initializes a new SQLite database,
	/// creates the `tiles` and `metadata` tables, and adds a unique index on tile coordinates.
	///
	/// # Errors
	/// Returns an error if the file cannot be removed, the database cannot be opened,
	/// or the schema creation fails.
	#[context("creating MBTilesWriter for '{}'", path.display())]
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

	/// Add multiple tiles to the MBTiles file within a single transaction.
	///
	/// Converts tile coordinates from XYZ to TMS indexing (`tile_row = 2^z - 1 - y`)
	/// before insertion, ensuring MBTiles compatibility.
	///
	/// # Errors
	/// Returns an error if the transaction or any insertion fails.
	#[context("adding {} tiles to MBTiles database", tiles.len())]
	fn add_tiles(&mut self, tiles: &Vec<(TileCoord, Blob)>) -> Result<()> {
		let mut conn = self.pool.get()?;
		let transaction = conn.transaction()?;
		for (c, blob) in tiles {
			let max_index = 2u32.pow(c.level as u32) - 1;
			transaction.execute(
				"INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
				params![c.level, c.x, max_index - c.y, blob.as_slice()],
			)?;
		}
		transaction.commit()?;
		Ok(())
	}

	/// Insert or replace a metadata key-value pair in the MBTiles database.
	///
	/// Used to populate the `metadata` table with dataset information such as
	/// bounds, minzoom, maxzoom, and format.
	///
	/// # Errors
	/// Returns an error if the statement execution fails.
	#[context("setting metadata key '{}' = '{}'", name, value)]
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
	/// Write all tiles and metadata from the given reader into an MBTiles file.
	///
	/// This method:
	/// - Creates a new SQLite database at `path` (removing any existing file).
	/// - Inserts metadata such as bounds, zoom range, and vector layers.
	/// - Writes all tiles from `reader`, flipping coordinates from XYZ to TMS.
	/// - Enforces MBTiles-compatible format and compression combinations.
	///
	/// # Errors
	/// Returns an error if writing fails, if an unsupported format/compression is used,
	/// or if database insertion encounters an error.
	#[context("writing MBTiles to '{}'", path.display())]
	async fn write_to_path(reader: &mut dyn TileSource, path: &Path, runtime: TilesRuntime) -> Result<()> {
		use TileCompression::*;
		use TileFormat::*;

		let writer = MBTilesWriter::new(path)?;

		let parameters = reader.metadata().clone();

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
		let pyramid = &reader.metadata().bbox_pyramid;
		let bbox = pyramid.get_geo_bbox().unwrap();
		let center = pyramid.get_geo_center().unwrap();
		let zoom_min = pyramid.get_level_min().unwrap();
		let zoom_max = pyramid.get_level_max().unwrap();
		writer.set_metadata(
			"bounds",
			&format!("{},{},{},{}", bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max),
		)?;
		writer.set_metadata("center", &format!("{},{},{}", center.0, center.1, center.2))?;
		writer.set_metadata("minzoom", &zoom_min.to_string())?;
		writer.set_metadata("maxzoom", &zoom_max.to_string())?;

		let tilejson = reader.tilejson();
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

		let writer_mutex = Arc::new(Mutex::new(writer));
		let tile_compression = reader.metadata().tile_compression;

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, stream| {
					let writer_mutex = Arc::clone(&writer_mutex);
					Box::pin(async move {
						let mut writer = writer_mutex.lock().await;
						stream
							.map_item_parallel(move |tile| tile.into_blob(tile_compression))
							.for_each_buffered(4096, |v| {
								writer.add_tiles(&v).unwrap();
							})
							.await;
						Ok(())
					})
				},
				runtime.clone(),
				None,
			)
			.await?;

		Ok(())
	}

	/// Not implemented: MBTiles cannot be streamed to a generic writer.
	///
	/// # Errors
	/// Always returns `not implemented`.
	#[context("writing MBTiles to generic writer")]
	async fn write_to_writer(
		_reader: &mut dyn TileSource,
		_writer: &mut dyn DataWriterTrait,
		_runtime: TilesRuntime,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MBTilesReader, MockReader, MockWriter, TileSourceMetadata};
	use assert_fs::NamedTempFile;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_full(5),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
			traversal: Traversal::ANY,
		})?;

		let filename = NamedTempFile::new("temp.mbtiles")?;
		MBTilesWriter::write_to_path(&mut mock_reader, &filename, TilesRuntime::default()).await?;

		let mut reader = MBTilesReader::open_path(&filename, TilesRuntime::default())?;

		MockWriter::write(&mut reader).await?;

		Ok(())
	}
}
