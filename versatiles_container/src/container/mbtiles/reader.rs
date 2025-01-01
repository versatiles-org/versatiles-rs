//! Provides functionality for reading tile data from an MBTiles SQLite database.
//!
//! The `MBTilesReader` struct is the primary component of this module, offering methods to read metadata and tile data from an MBTiles SQLite database.
//!
//! ## Features
//! - Supports reading metadata and tile data in multiple formats and compressions
//! - Provides methods to query the database for tile data based on coordinates or bounding boxes
//! - Allows overriding the tile compression method
//!
//! ## Usage Example
//! ```rust
//! use versatiles::container::MBTilesReader;
//! use versatiles::types::{Blob, TileCoord3, TilesReaderTrait};
//! use anyhow::Result;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Open the MBTiles database
//!     let path = std::env::current_dir()?.join("../testdata/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(&path)?;
//!
//!     // Get metadata
//!     if let Some(meta) = reader.get_meta()? {
//!         println!("Metadata: {:?}", meta);
//!     }
//!
//!     // Get tile data for specific coordinates
//!     let coord = TileCoord3::new(1, 1, 1)?;
//!     if let Some(tile_data) = reader.get_tile_data(&coord).await? {
//!         println!("Tile data: {:?}", tile_data);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if the database file does not exist, if the path is not absolute, or if there are issues querying the database.
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of reading metadata, handling different file formats, and verifying tile data.

use anyhow::{anyhow, ensure, Result};
use async_trait::async_trait;
use log::trace;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;
use versatiles_core::{
	types::{TileCompression::*, TileFormat::*, *},
	utils::{progress::get_progress_bar, JsonObject, JsonValue, TileJSON, TransformCoord},
};

/// A struct that provides functionality to read tile data from an MBTiles SQLite database.
pub struct MBTilesReader {
	name: String,
	pool: Pool<SqliteConnectionManager>,
	meta_data: Option<Blob>,
	parameters: TilesReaderParameters,
}

impl MBTilesReader {
	/// Opens the SQLite database and creates an `MBTilesReader` instance.
	///
	/// # Arguments
	/// * `path` - The path to the SQLite database file.
	///
	/// # Errors
	/// Returns an error if the file does not exist, if the path is not absolute, or if there is an error loading from SQLite.
	pub fn open_path(path: &Path) -> Result<MBTilesReader> {
		trace!("open {path:?}");

		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		MBTilesReader::load_from_sqlite(path)
	}

	/// Loads the MBTiles data from the SQLite database.
	///
	/// # Arguments
	/// * `path` - The path to the SQLite database file.
	///
	/// # Errors
	/// Returns an error if there is an issue connecting to the database or loading metadata.
	fn load_from_sqlite(path: &Path) -> Result<MBTilesReader> {
		trace!("load_from_sqlite {:?}", path);

		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;
		let parameters = TilesReaderParameters::new(PBF, Uncompressed, TileBBoxPyramid::new_empty());

		let mut reader = MBTilesReader {
			name: String::from(path.to_str().unwrap()),
			pool,
			meta_data: None,
			parameters,
		};

		reader.load_meta_data()?;

		Ok(reader)
	}

	/// Loads the metadata from the MBTiles database.
	///
	/// # Errors
	/// Returns an error if the tile format or compression is not specified or if there is an issue querying the database.
	fn load_meta_data(&mut self) -> Result<()> {
		trace!("load_meta_data");

		let pyramide = self.get_bbox_pyramid()?;
		let conn = self.pool.get()?;
		let mut stmt = conn.prepare("SELECT name, value FROM metadata")?;
		let entries = stmt.query_map([], |row| {
			Ok(RecordMetadata {
				name: row.get(0)?,
				value: row.get(1)?,
			})
		})?;

		let mut tile_format: Result<TileFormat> = Err(anyhow!(
			"mbtiles file {} does not specify tile format",
			self.name
		));
		let mut compression: Result<TileCompression> = Err(anyhow!(
			"mbtiles file {} does not specify compression",
			self.name
		));

		let mut meta = TileJSON::default();

		for entry in entries {
			let entry = entry?;
			let key = entry.name.as_str();
			let value = entry.value.as_str();
			match key {
				"format" => match value {
					"jpg" => {
						tile_format = Ok(JPG);
						compression = Ok(Uncompressed);
					}
					"pbf" => {
						tile_format = Ok(PBF);
						compression = Ok(Gzip);
					}
					"png" => {
						tile_format = Ok(PNG);
						compression = Ok(Uncompressed);
					}
					"webp" => {
						tile_format = Ok(WEBP);
						compression = Ok(Uncompressed);
					}
					_ => panic!("unknown file format: {}", value),
				},
				"json" => meta.add(JsonObject::parse_str(value)?)?,
				"attribution" => meta.attribution = Some(value.to_owned()),
				"author" => meta.author = Some(value.to_owned()),
				"license" => meta.license = Some(value.to_owned()),
				"name" => meta.name = Some(value.to_owned()),
				"type" => meta.type = Some(value.to_owned()),
				"version" => meta.version = Some(value.to_owned()),
				_ => {}
			}
		}

		self.meta_data = Some(Blob::from(meta.stringify()));

		self.parameters.tile_format = tile_format?;
		self.parameters.tile_compression = compression?;
		self.parameters.bbox_pyramid = pyramide;

		Ok(())
	}

	/// Executes a simple query on the MBTiles database.
	///
	/// # Arguments
	/// * `sql1` - The SQL query to execute.
	/// * `sql2` - Additional SQL conditions.
	///
	/// # Errors
	/// Returns an error if there is an issue executing the query.
	fn simple_query(&self, sql_value: &str, sql_where: &str) -> Result<i32> {
		let sql = if sql_where.is_empty() {
			format!("SELECT {sql_value} FROM tiles")
		} else {
			format!("SELECT {sql_value} FROM tiles WHERE {sql_where}")
		};

		trace!("SQL: {}", sql);

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(&sql)?;
		Ok(stmt.query_row([], |row| row.get::<_, i32>(0))?)
	}

	/// Gets the bounding box pyramid from the MBTiles database.
	///
	/// # Errors
	/// Returns an error if there is an issue querying the database.
	fn get_bbox_pyramid(&self) -> Result<TileBBoxPyramid> {
		trace!("get_bbox_pyramid");

		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		let z0 = self.simple_query("MIN(zoom_level)", "")?;
		let z1 = self.simple_query("MAX(zoom_level)", "")?;

		let mut progress = get_progress_bar("get mbtiles bbox pyramid", (z1 - z0 + 1) as u64);

		for z in z0..=z1 {
			let x0 = self.simple_query("MIN(tile_column)", &format!("zoom_level = {z}"))?;
			let x1 = self.simple_query("MAX(tile_column)", &format!("zoom_level = {z}"))?;
			let xc = (x0 + x1) / 2;

			/*
				SQLite is not very fast. In particular, the following query is slow for very large tables:
				> SELECT MIN(tile_row) FROM tiles WHERE zoom_level = 14

				The above query takes about 1 second per 1 million records to execute.
				For some reason SQLite is not using the index properly.

				The manual states: The MIN/MAX aggregate function can be optimised down to "a single index lookup",
				if it is the "leftmost column of an index": https://www.sqlite.org/optoverview.html#minmax
				I suspect that optimising for the rightmost column in an index (here: tile_row) does not work well.

				To increase the speed of the above query by a factor of about 10, we split it into 2 queries.

				The first query gives a good estimate by calculating MIN(tile_row) for the middle (or any other used) tile_column:
				> SELECT MIN(tile_row) FROM tiles WHERE zoom_level = 14 AND tile_column = $known_columns
				This takes only a few milliseconds.

				The second query calculates MIN(tile_row) for all columns, but starting with the estimate:
				> SELECT MIN(tile_row) FROM tiles WHERE zoom_level = 14 AND tile_row <= $min_row_estimate

				This seems to be a great help. I suspect it helps SQLite so it doesn't have to scan the entire index/table.
			*/

			let sql_prefix = format!("zoom_level = {z} AND");
			let columns = format!("(tile_column = {x0} OR tile_column = {xc} OR tile_column = {x1})");

			let mut y0 = self.simple_query("MIN(tile_row)", &format!("{sql_prefix} {columns}"))?;
			let mut y1 = self.simple_query("MAX(tile_row)", &format!("{sql_prefix} {columns}"))?;

			y0 = self.simple_query("MIN(tile_row)", &format!("{sql_prefix} tile_row <= {y0}"))?;
			y1 = self.simple_query("MAX(tile_row)", &format!("{sql_prefix} tile_row >= {y1}"))?;

			let max_value = 2i32.pow(z as u32) - 1;

			bbox_pyramid.set_level_bbox(TileBBox::new(
				z as u8,
				x0.clamp(0, max_value) as u32,
				y0.clamp(0, max_value) as u32,
				x1.clamp(0, max_value) as u32,
				y1.clamp(0, max_value) as u32,
			)?);

			progress.inc(1);
		}

		progress.finish();

		bbox_pyramid.flip_y();

		Ok(bbox_pyramid)
	}
}

#[async_trait]
impl TilesReaderTrait for MBTilesReader {
	/// Returns the container name.
	fn get_container_name(&self) -> &str {
		"mbtiles"
	}

	/// Returns the metadata as a `Blob`.
	///
	/// # Errors
	/// Returns an error if there is an issue retrieving the metadata.
	fn get_meta(&self) -> Result<Option<&TileJSON>> {
		Ok(self.meta_data.as_ref())
	}

	/// Returns the parameters of the tiles reader.
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Overrides the tile compression method.
	///
	/// # Arguments
	/// * `tile_compression` - The new tile compression method.
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}

	/// Returns the tile data for the specified coordinates as a `Blob`.
	///
	/// # Arguments
	/// * `coord` - The coordinates of the tile.
	///
	/// # Errors
	/// Returns an error if there is an issue retrieving the tile data.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		trace!("read tile from coord {coord:?}");

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(
			"SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?",
		)?;

		let max_index = 2u32.pow(coord.z as u32) - 1;
		if let Ok(vec) = stmt.query_row([coord.x, max_index - coord.y, coord.z as u32], |row| {
			row.get::<_, Vec<u8>>(0)
		}) {
			Ok(Some(Blob::from(vec)))
		} else {
			Ok(None)
		}
	}

	/// Returns a stream of tile data for the specified bounding box.
	///
	/// # Arguments
	/// * `bbox` - The bounding box of the tiles.
	///
	/// # Errors
	/// Returns an error if there is an issue querying the database.
	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		trace!("read tile stream from bbox {bbox:?}");

		if bbox.is_empty() {
			return TileStream::new_empty();
		}

		let max_index = bbox.max;

		trace!("corrected bbox {bbox:?}");

		let conn = self.pool.get().unwrap();
		let mut stmt = conn
			 .prepare(
					"SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
			 )
			 .unwrap();

		let vec: Vec<(TileCoord3, Blob)> = stmt
			.query_map(
				[
					bbox.x_min,
					bbox.x_max,
					max_index - bbox.y_max,
					max_index - bbox.y_min,
					bbox.level as u32,
				],
				move |row| {
					let coord = TileCoord3::new(
						row.get::<_, u32>(0)?,
						max_index - row.get::<_, u32>(1)?,
						row.get::<_, u8>(2)?,
					)
					.unwrap();
					let blob = Blob::from(row.get::<_, Vec<u8>>(3)?);
					Ok((coord, blob))
				},
			)
			.unwrap()
			.filter_map(|r| match r {
				Ok(ok) => Some(ok),
				Err(_) => None,
			})
			.collect();

		trace!("got {} tiles", vec.len());

		TileStream::from_vec(vec)
	}

	/// Returns the name of the MBTiles database.
	fn get_source_name(&self) -> &str {
		&self.name
	}
}

impl std::fmt::Debug for MBTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MBTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

/// A struct representing a metadata record in the MBTiles database.
struct RecordMetadata {
	name: String,
	value: String,
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::container::MockTilesWriter;
	use lazy_static::lazy_static;
	use std::{env, path::PathBuf};

	lazy_static! {
		static ref PATH: PathBuf = env::current_dir()
			.unwrap()
			.join("../testdata/berlin.mbtiles");
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = MBTilesReader::open_path(&PATH)?;

		assert_eq!(format!("{:?}", reader), "MBTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [1,0,1,0] (1), 2: [2,1,2,1] (1), 3: [4,2,4,2] (1), 4: [8,5,8,5] (1), 5: [17,10,17,10] (1), 6: [34,20,34,21] (2), 7: [68,41,68,42] (2), 8: [137,83,137,84] (2), 9: [274,167,275,168] (4), 10: [549,335,551,336] (6), 11: [1098,670,1102,673] (20), 12: [2196,1340,2204,1346] (63), 13: [4393,2680,4409,2693] (238), 14: [8787,5361,8818,5387] (864)], tile_compression: Gzip, tile_format: PBF } }");
		assert_eq!(reader.get_container_name(), "mbtiles");
		assert!(reader
			.get_source_name()
			.ends_with("../testdata/berlin.mbtiles"));
		assert_eq!(reader.get_meta()?.unwrap().as_str(),  "{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",\"description\":\"Tile config for simple vector tiles schema\",\"license\":\"Open Database License 1.0\",\"name\":\"Tilemaker to Geofabrik Vector Tiles schema\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"dummy\":\"Number\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":7},{\"fields\":{\"kind\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"3.0\"}");
		assert_eq!(format!("{:?}", reader.get_parameters()), "TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [1,0,1,0] (1), 2: [2,1,2,1] (1), 3: [4,2,4,2] (1), 4: [8,5,8,5] (1), 5: [17,10,17,10] (1), 6: [34,20,34,21] (2), 7: [68,41,68,42] (2), 8: [137,83,137,84] (2), 9: [274,167,275,168] (4), 10: [549,335,551,336] (6), 11: [1098,670,1102,673] (20), 12: [2196,1340,2204,1346] (63), 13: [4393,2680,4409,2693] (238), 14: [8787,5361,8818,5387] (864)], tile_compression: Gzip, tile_format: PBF }");
		assert_eq!(reader.get_parameters().tile_compression, Gzip);
		assert_eq!(reader.get_parameters().tile_format, PBF);

		let tile = reader
			.get_tile_data(&TileCoord3::new(8803, 5376, 14)?)
			.await?
			.unwrap();
		assert_eq!(tile.len(), 172969);
		assert_eq!(tile.get_range(0..10), &[31, 139, 8, 0, 0, 0, 0, 0, 0, 3]);
		assert_eq!(
			tile.get_range(172959..172969),
			&[255, 15, 172, 89, 205, 237, 7, 134, 5, 0]
		);

		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	// Test tile fetching
	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		use versatiles_core::utils::PrettyPrint;

		let mut reader = MBTilesReader::open_path(&PATH)?;

		let mut printer = PrettyPrint::new();
		reader
			.probe_container(&printer.get_category("container").await)
			.await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n   deep container probing is not implemented for this container format\n"
		);

		let mut printer = PrettyPrint::new();
		reader
			.probe_tiles(&printer.get_category("tiles").await)
			.await?;
		assert_eq!(
			printer.as_string().await,
			"tiles:\n   deep tiles probing is not implemented for this container format\n"
		);

		Ok(())
	}
}
