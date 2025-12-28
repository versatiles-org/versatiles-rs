//! Read tiles and metadata from an MBTiles (SQLite) database.
//!
//! The `MBTilesReader` loads TileJSON-style metadata from the MBTiles `metadata` table
//! and fetches tile blobs from the `tiles` table. It derives the tile **format** and
//! **compression** primarily from the `format` field (per the Mapbox MBTiles 1.3 spec):
//!
//! - `format = "png"` → `TileFormat::PNG` + `TileCompression::Uncompressed`
//! - `format = "jpg"` → `TileFormat::JPG` + `TileCompression::Uncompressed`
//! - `format = "webp"` → `TileFormat::WEBP` + `TileCompression::Uncompressed`
//! - `format = "pbf"` → `TileFormat::MVT`  + `TileCompression::Gzip`
//!
//! It also reads optional fields like `bounds`, `minzoom`, `maxzoom`, and `json` (for
//! `vector_layers`) and merges them into an internal [`TileJSON`](versatiles_core::TileJSON).
//! The bounding-box pyramid is inferred from the `tiles` table to augment/validate metadata.
//!
//! ## Requirements
//! - The MBTiles file **must be an absolute path** when opening with [`open_path`].
//! - The database must include a `format` entry in `metadata` so that format & compression
//!   can be determined.
//!
//! ## Usage
//! ```rust,no_run
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use anyhow::Result;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let runtime = TilesRuntime::default();
//!
//!     // Use an absolute path
//!     let path = Path::new("/absolute/path/to/berlin.mbtiles");
//!     let mut reader = MBTilesReader::open_path(path, runtime)?;
//!
//!     // Inspect metadata
//!     let tj: &TileJSON = reader.tilejson();
//!
//!     // Fetch a single tile (z/x/y)
//!     let coord = TileCoord::new(1, 1, 1)?;
//!     if let Some(tile) = reader.get_tile(&coord).await? {
//!         let _blob = tile.into_blob(reader.metadata().tile_compression)?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if the path is not absolute or the file does not exist.
//! - Returns errors if the database is unreadable, the `format` is missing/unknown,
//!   or queries fail.

use crate::{SourceType, Tile, TileSourceMetadata, TileSourceTrait, TilesRuntime, Traversal};
use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::{path::Path, sync::Arc};
use versatiles_core::{TileCompression::*, TileFormat::*, json::parse_json_str, types::*};
use versatiles_derive::context;

/// Reader for MBTiles (SQLite) containers.
///
/// Opens a SQLite database with `metadata` and `tiles` tables, merges metadata into
/// [`TileJSON`], infers a bounding-box pyramid by scanning levels/rows/columns, and
/// exposes tiles via the [`TileSourceTrait`] interface.
pub struct MBTilesReader {
	name: String,
	pool: Pool<SqliteConnectionManager>,
	tilejson: TileJSON,
	metadata: TileSourceMetadata,
	runtime: TilesRuntime,
}

impl MBTilesReader {
	/// Opens the SQLite database and creates an `MBTilesReader` instance.
	///
	/// Open an MBTiles database from an **absolute** filesystem path.
	///
	/// Validates existence and absoluteness of `path`, then initializes a connection pool
	/// and loads metadata/parameters.
	///
	/// # Errors
	/// Returns an error if the file does not exist, the path is not absolute, or SQLite cannot be opened.
	#[context("opening MBTiles at '{}'", path.display())]
	pub fn open_path(path: &Path, runtime: TilesRuntime) -> Result<MBTilesReader> {
		log::debug!("open {path:?}");

		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		MBTilesReader::load_from_sqlite(path, runtime)
	}

	/// Internal loader that establishes the SQLite pool, sets default parameters,
	/// and then calls [`load_meta_data`] to populate `tilejson` and parameters.
	///
	/// # Errors
	/// Returns an error if the connection cannot be established or metadata fails to load.
	#[context("loading SQLite '{}'", path.display())]
	fn load_from_sqlite(path: &Path, runtime: TilesRuntime) -> Result<MBTilesReader> {
		log::debug!("load_from_sqlite {path:?}");

		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;
		let metadata = TileSourceMetadata::new(MVT, Uncompressed, TileBBoxPyramid::new_empty(), Traversal::ANY);

		let mut reader = MBTilesReader {
			name: String::from(path.to_str().unwrap()),
			pool,
			tilejson: TileJSON::default(),
			metadata,
			runtime,
		};

		reader.load_meta_data()?;

		Ok(reader)
	}

	/// Read and merge MBTiles metadata.
	///
	/// Parses `format` to determine tile format & transport compression, reads `bounds`,
	/// `minzoom`, `maxzoom`, and `json` (for `vector_layers`), then merges them into `tilejson`.
	/// Also updates the bounding-box pyramid from the database.
	///
	/// # Errors
	/// Returns an error if `format` is missing/unknown or queries fail.
	#[context("loading MBTiles metadata from '{}'", self.name)]
	fn load_meta_data(&mut self) -> Result<()> {
		log::debug!("load_meta_data");

		let pyramid = self.get_bbox_pyramid()?;
		let conn = self.pool.get()?;
		let mut stmt = conn.prepare("SELECT name, value FROM metadata")?;
		let entries = stmt.query_map([], |row| {
			Ok(RecordMetadata {
				name: row.get(0)?,
				value: row.get(1)?,
			})
		})?;

		let mut tile_format: Result<TileFormat> = Err(anyhow!("mbtiles file {} does not specify tile format", self.name));
		let mut compression: Result<TileCompression> =
			Err(anyhow!("mbtiles file {} does not specify compression", self.name));

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
						tile_format = Ok(MVT);
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
					_ => panic!("unknown file format: {value}"),
				},
				// https://github.com/mapbox/mbtiles-spec/blob/master/1.3/spec.md#content
				"bounds" => {
					let bounds = value
						.split(',')
						.map(|s| s.parse::<f64>())
						.collect::<Result<Vec<f64>, _>>()?;
					self.tilejson.limit_bbox(GeoBBox::try_from(bounds)?);
				}
				"name" | "attribution" | "author" | "description" | "license" | "type" | "version" => {
					self.tilejson.set_string(key, value)?
				}
				"minzoom" | "maxzoom" => self.tilejson.set_byte(key, value.parse::<u8>()?)?,
				"json" => {
					let json = parse_json_str(value).with_context(|| format!("failed to parse JSON: {value}"))?;
					let object = json.as_object().with_context(|| anyhow!("expected JSON object"))?;
					let vector_layers = object
						.get("vector_layers")
						.with_context(|| anyhow!("expected 'vector_layers'"))?;
					self.tilejson.set_vector_layers(vector_layers)?;
				}
				_ => {}
			}
		}

		self.tilejson.update_from_pyramid(&pyramid);
		self.metadata.tile_format = tile_format?;
		self.metadata.tile_compression = compression?;
		self.metadata.bbox_pyramid = pyramid;

		Ok(())
	}

	/// Execute a simple aggregate query against the `tiles` table.
	///
	/// * `sql_value` — the SELECT expression (e.g., `MIN(tile_column)`).
	/// * `sql_where` — an optional WHERE clause without the `WHERE` keyword.
	///
	/// # Errors
	/// Returns an error if executing the query fails.
	#[context("executing tiles query")]
	fn simple_query(&self, sql_value: &str, sql_where: &str) -> Result<i32> {
		let sql = if sql_where.is_empty() {
			format!("SELECT {sql_value} FROM tiles")
		} else {
			format!("SELECT {sql_value} FROM tiles WHERE {sql_where}")
		};

		log::trace!("SQL: {sql}");

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(&sql)?;
		Ok(stmt.query_row([], |row| row.get::<_, i32>(0))?)
	}

	/// Compute the per-zoom bounding boxes from the `tiles` table.
	///
	/// Uses a two-step MIN/MAX strategy to speed up queries on large tables by estimating
	/// bounds from a few columns before querying the constrained range. Flips Y afterward
	/// to match XYZ addressing.
	///
	/// # Errors
	/// Returns an error if queries fail.
	#[context("computing bbox pyramid from MBTiles")]
	fn get_bbox_pyramid(&self) -> Result<TileBBoxPyramid> {
		log::debug!("get_bbox_pyramid");

		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		let z0 = self.simple_query("MIN(zoom_level)", "")?;
		let z1 = self.simple_query("MAX(zoom_level)", "")?;

		let progress = self
			.runtime
			.create_progress("get mbtiles bbox pyramid", (z1 - z0 + 1) as u64);

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

			bbox_pyramid.set_level_bbox(TileBBox::from_min_and_max(
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
impl TileSourceTrait for MBTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("mbtiles", &self.name)
	}

	/// Return the TileJSON metadata view for this dataset.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Returns the parameters of the tiles reader.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Fetch a single tile by XYZ coordinate.
	///
	/// Coordinates are converted to TMS row indexing internally (via `y' = 2^z - 1 - y`).
	/// Returns `Ok(None)` when the tile is not present.
	///
	/// # Errors
	/// Returns an error if the query fails.
	#[context("fetching tile {:?} from '{}'", coord, self.name)]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		log::trace!("read tile from coord {coord:?}");

		let conn = self.pool.get()?;
		let mut stmt =
			conn.prepare("SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?")?;

		let max_index = 2u32.pow(coord.level as u32) - 1;
		if let Ok(vec) = stmt.query_row([coord.x, max_index - coord.y, coord.level as u32], |row| {
			row.get::<_, Vec<u8>>(0)
		}) {
			Ok(Some(Tile::from_blob(
				Blob::from(vec),
				self.metadata.tile_compression,
				self.metadata.tile_format,
			)))
		} else {
			Ok(None)
		}
	}

	/// Stream tiles within a single-zoom bounding box.
	///
	/// The input bbox is XYZ; rows are flipped to TMS for the query and flipped back on output.
	/// Empty bboxes yield an empty stream.
	///
	/// # Errors
	/// Returns an error if the query fails.
	#[context("streaming tiles for bbox {:?}", bbox)]
	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);

		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		bbox.flip_y();

		log::trace!("corrected bbox {bbox:?}");

		let conn = self.pool.get().unwrap();
		let mut stmt = conn
			 .prepare(
					"SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
			 )
			 .unwrap();

		let vec: Vec<(TileCoord, Tile)> = stmt
			.query_map(
				[
					bbox.x_min()?,
					bbox.x_max()?,
					bbox.y_min()?,
					bbox.y_max()?,
					bbox.level as u32,
				],
				move |row| {
					let x = row.get::<_, u32>(0)?;
					let y = row.get::<_, u32>(1)?;
					let level = row.get::<_, u8>(2)?;
					let mut coord = TileCoord::new(level, x, y).unwrap();
					coord.flip_y();
					let blob = Blob::from(row.get::<_, Vec<u8>>(3)?);
					let tile = Tile::from_blob(blob, self.metadata.tile_compression, self.metadata.tile_format);
					Ok((coord, tile))
				},
			)
			.unwrap()
			.filter_map(|r| r.ok())
			.collect();

		log::trace!("got {} tiles", vec.len());

		Ok(TileStream::from_vec(vec))
	}
}

impl std::fmt::Debug for MBTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MBTilesReader")
			.field("parameters", &self.metadata())
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
	use crate::MockTilesWriter;
	use lazy_static::lazy_static;
	use std::{env, path::PathBuf};

	lazy_static! {
		static ref PATH: PathBuf = env::current_dir().unwrap().join("../testdata/berlin.mbtiles");
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = MBTilesReader::open_path(&PATH, TilesRuntime::default())?;

		assert_eq!(
			format!("{reader:?}"),
			"MBTilesReader { parameters: TileSourceMetadata { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [1,0,1,0] (1x1), 2: [2,1,2,1] (1x1), 3: [4,2,4,2] (1x1), 4: [8,5,8,5] (1x1), 5: [17,10,17,10] (1x1), 6: [34,20,34,21] (1x2), 7: [68,41,68,42] (1x2), 8: [137,83,137,84] (1x2), 9: [274,167,275,168] (2x2), 10: [549,335,551,336] (3x2), 11: [1098,670,1102,673] (5x4), 12: [2196,1340,2204,1346] (9x7), 13: [4393,2680,4409,2693] (17x14), 14: [8787,5361,8818,5387] (32x27)], tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full) } }"
		);
		assert_eq!(
			reader.source_type().to_string(),
			format!("container 'mbtiles' ('{}')", PATH.to_str().unwrap())
		);
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",\"bounds\":[13.08283,52.33446,13.762245,52.6783],\"description\":\"Tile config for simple vector tiles schema\",\"license\":\"Open Database License 1.0\",\"maxzoom\":14,\"minzoom\":0,\"name\":\"Tilemaker to Geofabrik Vector Tiles schema\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"dummy\":\"Number\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":7},{\"fields\":{},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"kind\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"3.0\"}"
		);
		assert_eq!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { bbox_pyramid: [0: [0,0,0,0] (1x1), 1: [1,0,1,0] (1x1), 2: [2,1,2,1] (1x1), 3: [4,2,4,2] (1x1), 4: [8,5,8,5] (1x1), 5: [17,10,17,10] (1x1), 6: [34,20,34,21] (1x2), 7: [68,41,68,42] (1x2), 8: [137,83,137,84] (1x2), 9: [274,167,275,168] (2x2), 10: [549,335,551,336] (3x2), 11: [1098,670,1102,673] (5x4), 12: [2196,1340,2204,1346] (9x7), 13: [4393,2680,4409,2693] (17x14), 14: [8787,5361,8818,5387] (32x27)], tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full) }"
		);
		assert_eq!(reader.metadata().tile_compression, Gzip);
		assert_eq!(reader.metadata().tile_format, MVT);

		let tile = reader
			.get_tile(&TileCoord::new(14, 8803, 5376)?)
			.await?
			.unwrap()
			.into_blob(reader.metadata().tile_compression)?;
		assert_eq!(tile.len(), 172969);
		assert_eq!(tile.range(0..10), &[31, 139, 8, 0, 0, 0, 0, 0, 0, 3]);
		assert_eq!(tile.range(172959..172969), &[255, 15, 172, 89, 205, 237, 7, 134, 5, 0]);

		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	// Test tile fetching
	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		use versatiles_core::utils::PrettyPrint;

		let reader = MBTilesReader::open_path(&PATH, TilesRuntime::default())?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n  deep container probing is not implemented for this source\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await,
			"tiles:\n  deep tiles probing is not implemented for this source\n"
		);

		Ok(())
	}
}
