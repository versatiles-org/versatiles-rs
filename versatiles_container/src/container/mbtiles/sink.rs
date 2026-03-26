//! A [`TileSink`] implementation that writes tiles to an MBTiles (SQLite) database.
//!
//! Uses the same schema as [`MBTilesWriter`](super::MBTilesWriter): `tiles` and `metadata`
//! tables with TMS coordinate flipping (`tile_row = 2^z - 1 - y`).
//! Thread-safe via an internal `Mutex` around the tile insert buffer.

use crate::TileSink;
use anyhow::{Result, bail};
use r2d2::Pool;
use r2d2_sqlite::{SqliteConnectionManager, rusqlite::params};
use std::collections::HashSet;
use std::fs::remove_file;
use std::path::Path;
use std::sync::Mutex;
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, json::JsonObject};
use versatiles_derive::context;

const BUFFER_SIZE: usize = 4096;

/// A tile sink that writes pre-compressed blobs into an MBTiles SQLite database.
///
/// Constructed with a fixed `TileFormat` and `TileCompression`. Only the following
/// combinations are supported:
/// - `PNG` / `Uncompressed`
/// - `JPG` / `Uncompressed`
/// - `WEBP` / `Uncompressed`
/// - `MVT` / `Gzip`
///
/// # Thread Safety
///
/// Uses a `std::sync::Mutex` around an internal buffer. Tiles are batched and
/// flushed to SQLite in transactions of up to 4096 tiles for performance.
pub struct MBTilesTileSink {
	pool: Pool<SqliteConnectionManager>,
	format_str: String,
	buffer: Mutex<Vec<(TileCoord, Vec<u8>)>>,
	written: Mutex<HashSet<TileCoord>>,
}

impl MBTilesTileSink {
	/// Create a new `MBTilesTileSink` at the given path.
	///
	/// Removes any existing file, creates the SQLite database with `tiles` and `metadata` tables.
	///
	/// # Errors
	/// Returns an error if the format/compression combination is unsupported or if
	/// database creation fails.
	/// Open an MBTiles tile sink from a destination string.
	///
	/// MBTiles requires a local filesystem path (SFTP is not supported).
	pub fn open(
		destination: &str,
		tile_format: TileFormat,
		tile_compression: TileCompression,
		_runtime: &crate::TilesRuntime,
	) -> Result<Self> {
		if destination.starts_with("sftp://") {
			bail!("MBTiles does not support SFTP output (SQLite requires local filesystem)");
		}
		let path = std::env::current_dir()?.join(destination);
		Self::new(&path, tile_format, tile_compression)
	}

	#[context("creating MBTilesTileSink for '{}'", path.display())]
	fn new(path: &Path, tile_format: TileFormat, tile_compression: TileCompression) -> Result<Self> {
		use TileCompression::{Gzip, Uncompressed};
		use TileFormat::{JPG, MVT, PNG, WEBP};

		let format_str = match (tile_format, tile_compression) {
			(JPG, Uncompressed) => "jpg",
			(MVT, Gzip) => "pbf",
			(PNG, Uncompressed) => "png",
			(WEBP, Uncompressed) => "webp",
			_ => bail!(
				"combination of format ({tile_format}) and compression ({tile_compression}) is not supported. \
				 MBTiles supports only uncompressed jpg/png/webp or gzipped pbf"
			),
		};

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

		Ok(Self {
			pool,
			format_str: format_str.to_string(),
			buffer: Mutex::new(Vec::with_capacity(BUFFER_SIZE)),
			written: Mutex::new(HashSet::new()),
		})
	}

	/// Flush the internal buffer to the database in a single transaction.
	fn flush_buffer(&self, tiles: &[(TileCoord, Vec<u8>)]) -> Result<()> {
		if tiles.is_empty() {
			return Ok(());
		}
		let mut conn = self.pool.get()?;
		let transaction = conn.transaction()?;
		for (c, blob) in tiles {
			let max_index = 2u32.pow(u32::from(c.level)) - 1;
			transaction.execute(
				"INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
				params![c.level, c.x, max_index - c.y, blob.as_slice()],
			)?;
		}
		transaction.commit()?;
		Ok(())
	}

	/// Set a metadata key-value pair.
	fn set_metadata(&self, name: &str, value: &str) -> Result<()> {
		self.pool.get()?.execute(
			"INSERT OR REPLACE INTO metadata (name, value) VALUES (?1, ?2)",
			params![name, value],
		)?;
		Ok(())
	}

	/// Write all TileJSON metadata fields to the metadata table.
	fn write_tilejson_metadata(&self, tilejson: &TileJSON) -> Result<()> {
		self.set_metadata("format", &self.format_str)?;
		self.set_metadata("type", "baselayer")?;
		self.set_metadata("version", "3.0")?;

		if let Some(bbox) = tilejson.bounds {
			self.set_metadata(
				"bounds",
				&format!("{},{},{},{}", bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max),
			)?;
		}
		if let Some(center) = tilejson.center {
			self.set_metadata("center", &format!("{},{},{}", center.0, center.1, center.2))?;
		}
		if let Some(z) = tilejson.min_zoom() {
			self.set_metadata("minzoom", &z.to_string())?;
		}
		if let Some(z) = tilejson.max_zoom() {
			self.set_metadata("maxzoom", &z.to_string())?;
		}
		if let Some(vector_layers) = tilejson.as_object().get("vector_layers") {
			self.set_metadata(
				"json",
				&JsonObject::from(vec![("vector_layers", vector_layers)]).stringify(),
			)?;
		}

		for key in ["name", "author", "type", "description", "version", "license"] {
			if let Some(value) = tilejson.get_str(key) {
				self.set_metadata(key, value)?;
			}
		}

		Ok(())
	}
}

impl TileSink for MBTilesTileSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		if !self.written.lock().unwrap().insert(*coord) {
			return Ok(());
		}
		let mut buf = self.buffer.lock().unwrap();
		buf.push((*coord, blob.as_slice().to_vec()));

		if buf.len() >= BUFFER_SIZE {
			let tiles: Vec<_> = buf.drain(..).collect();
			drop(buf);
			self.flush_buffer(&tiles)?;
		}

		Ok(())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, _runtime: &crate::TilesRuntime) -> Result<()> {
		// Flush remaining tiles
		let remaining: Vec<_> = self.buffer.lock().unwrap().drain(..).collect();
		self.flush_buffer(&remaining)?;

		// Write metadata
		self.write_tilejson_metadata(tilejson)?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MBTilesReader, TileSource, TilesRuntime};

	#[test]
	fn write_and_read_back() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink.mbtiles")?;
		let runtime = TilesRuntime::default();

		let sink = MBTilesTileSink::open(
			temp.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		let coord = TileCoord::new(3, 1, 2)?;
		let blob = Blob::from(vec![0u8; 16]);
		sink.write_tile(&coord, &blob)?;

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_min_zoom(3);
		tilejson.set_max_zoom(3);
		Box::new(sink).finish(&tilejson, &crate::TilesRuntime::default())?;

		let reader = MBTilesReader::open(&temp, TilesRuntime::default())?;
		assert_eq!(reader.metadata().tile_format, TileFormat::PNG);
		assert_eq!(reader.metadata().bbox_pyramid.count_tiles(), 1);

		Ok(())
	}

	#[test]
	fn rejects_unsupported_format() {
		let temp = assert_fs::NamedTempFile::new("test_sink_bad.mbtiles").unwrap();
		let runtime = TilesRuntime::default();
		let result = MBTilesTileSink::open(
			temp.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Brotli,
			&runtime,
		);
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn write_multiple_and_read_back() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink_multi.mbtiles")?;
		let runtime = TilesRuntime::default();

		let sink = MBTilesTileSink::open(
			temp.to_str().unwrap(),
			TileFormat::WEBP,
			TileCompression::Uncompressed,
			&runtime,
		)?;

		for y in 0..4 {
			for x in 0..4 {
				let coord = TileCoord::new(2, x, y)?;
				#[allow(clippy::cast_possible_truncation)]
				let blob = Blob::from(vec![x as u8; 8]);
				sink.write_tile(&coord, &blob)?;
			}
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_min_zoom(2);
		tilejson.set_max_zoom(2);
		Box::new(sink).finish(&tilejson, &crate::TilesRuntime::default())?;

		let reader = MBTilesReader::open(&temp, TilesRuntime::default())?;
		assert_eq!(reader.metadata().tile_format, TileFormat::WEBP);
		assert_eq!(reader.metadata().bbox_pyramid.count_tiles(), 16);

		// Verify a specific tile via get_tile
		let tile = reader.get_tile(&TileCoord::new(2, 1, 1)?).await?;
		assert!(tile.is_some());
		let blob = tile.unwrap().into_blob(TileCompression::Uncompressed)?;
		assert_eq!(blob.as_slice(), &[1u8; 8]);

		Ok(())
	}
}
