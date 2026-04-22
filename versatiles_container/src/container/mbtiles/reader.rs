//! Read tiles and metadata from an `MBTiles` (`SQLite`) database.
//!
//! The `MBTilesReader` loads TileJSON-style metadata from the `MBTiles` `metadata` table
//! and fetches tile blobs from the `tiles` table. It derives the tile **format** and
//! **compression** primarily from the `format` field (per the Mapbox `MBTiles` 1.3 spec):
//!
//! - `format = "png"` → `TileFormat::PNG` + `TileCompression::Uncompressed`
//! - `format = "jpg"` → `TileFormat::JPG` + `TileCompression::Uncompressed`
//! - `format = "webp"` → `TileFormat::WEBP` + `TileCompression::Uncompressed`
//! - `format = "pbf"` → `TileFormat::MVT`  + `TileCompression::Gzip`
//!
//! It also reads optional fields like `bounds`, `minzoom`, `maxzoom`, and `json` (for
//! `vector_layers`) and merges them into an internal [`TileJSON`](versatiles_core::TileJSON).
//!
//! The per-level coverage pyramid is **not** scanned at open time. It is computed
//! lazily on the first call to [`TileSource::tile_pyramid`](crate::TileSource::tile_pyramid)
//! by reading `(zoom_level, tile_column, tile_row)` from the `tiles` table, and
//! the result is cached for subsequent calls.
//!
//! ## Requirements
//! - The `MBTiles` file **must be an absolute path** when opening with [`open`].
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
//!     let mut reader = MBTilesReader::open(path, runtime)?;
//!
//!     // Inspect metadata
//!     let tj: &TileJSON = reader.tilejson();
//!
//!     // Fetch a single tile (z/x/y)
//!     let coord = TileCoord::new(1, 1, 1)?;
//!     if let Some(tile) = reader.tile(&coord).await? {
//!         let _blob = tile.into_blob(reader.metadata().tile_compression())?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Errors
//! - Returns errors if the path is not absolute or the file does not exist.
//! - Returns errors if the database is unreadable, the `format` is missing/unknown,
//!   or queries fail.

use crate::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesReader, TilesRuntime, Traversal};
use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::{path::Path, sync::Arc};
use versatiles_core::{
	TileCompression::{Gzip, Uncompressed},
	TileFormat::{JPG, MVT, PNG, WEBP},
	json::parse_json_str,
	types::{
		Blob, GeoBBox, GeoCenter, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream,
	},
};
use versatiles_derive::context;

#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;

/// Reader for `MBTiles` (`SQLite`) containers.
///
/// Opens a `SQLite` database with `metadata` and `tiles` tables, merges the
/// metadata rows into [`TileJSON`], and exposes tiles via the [`TileSource`]
/// interface. The coverage pyramid is computed lazily on first request via
/// [`TileSource::tile_pyramid`] and cached thereafter.
pub struct MBTilesReader {
	name: String,
	pool: Pool<SqliteConnectionManager>,
	tilejson: TileJSON,
	metadata: TileSourceMetadata,
	#[allow(dead_code)]
	runtime: TilesRuntime,
}

impl MBTilesReader {
	/// Opens the `SQLite` database and creates an `MBTilesReader` instance.
	///
	/// Open an `MBTiles` database from an **absolute** filesystem path.
	///
	/// Validates existence and absoluteness of `path`, then initializes a connection pool
	/// and loads metadata/parameters.
	///
	/// # Errors
	/// Returns an error if the file does not exist, the path is not absolute, or `SQLite` cannot be opened.
	#[context("opening MBTiles at '{}'", path.display())]
	pub fn open(path: &Path, runtime: TilesRuntime) -> Result<MBTilesReader> {
		log::debug!("open {path:?}");

		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		MBTilesReader::load_from_sqlite(path, runtime)
	}

	/// Internal loader that establishes the `SQLite` pool, sets default parameters,
	/// and then calls [`load_meta_data`] to populate `tilejson` and parameters.
	///
	/// # Errors
	/// Returns an error if the connection cannot be established or metadata fails to load.
	#[context("loading SQLite '{}'", path.display())]
	fn load_from_sqlite(path: &Path, runtime: TilesRuntime) -> Result<MBTilesReader> {
		log::debug!("load_from_sqlite {path:?}");

		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;
		let metadata = TileSourceMetadata::new(MVT, Uncompressed, Traversal::ANY, None);

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

	/// Read and merge `MBTiles` metadata.
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
						.map(str::parse::<f64>)
						.collect::<Result<Vec<f64>, _>>()?;
					self.tilejson.limit_bbox(GeoBBox::try_from(bounds)?);
				}
				"name" | "attribution" | "author" | "description" | "license" | "type" | "version" => {
					self.tilejson.set_string(key, value)?;
				}
				"center" => {
					let parts = value
						.split(',')
						.map(str::parse::<f64>)
						.collect::<Result<Vec<f64>, _>>()?;
					self.tilejson.center = Some(GeoCenter::try_from(parts)?);
				}
				"minzoom" => self.tilejson.set_zoom_min(value.parse::<u8>()?),
				"maxzoom" => self.tilejson.set_zoom_max(value.parse::<u8>()?),
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

		self.metadata.set_tile_format(tile_format?);
		self.metadata.set_tile_compression(compression?);

		Ok(())
	}

	/// Compute the exact tile coverage pyramid from the `tiles` table.
	///
	/// Reads every `(zoom_level, tile_column, tile_row)` row in a single scan,
	/// converts them to [`TileCoord`]s, and builds an exact [`TilePyramid`] via
	/// [`TilePyramid::from_tile_coords`]. Flips Y afterward to convert from TMS
	/// to XYZ addressing.
	///
	/// # Errors
	/// Returns an error if the query fails.
	#[context("computing bbox pyramid from MBTiles")]
	fn bbox_pyramid(&self) -> Result<TilePyramid> {
		log::debug!("bbox_pyramid");

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare("SELECT zoom_level, tile_column, tile_row FROM tiles")?;
		let coords: Vec<TileCoord> = stmt
			.query_map([], |row| {
				Ok((row.get::<_, u8>(0)?, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?))
			})?
			.filter_map(Result::ok)
			.filter_map(|(z, x, y)| TileCoord::new(z, x, y).ok())
			.collect();

		let mut pyramid = TilePyramid::from_tile_coords(coords.into_iter());
		pyramid.flip_y();
		Ok(pyramid)
	}
}

#[async_trait]
impl TilesReader for MBTilesReader {
	fn supports_data_reader() -> bool {
		false
	}

	async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<SharedTileSource> {
		Ok(Self::open(path, runtime)?.into_shared())
	}
}

#[async_trait]
impl TileSource for MBTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("mbtiles", &self.name)
	}

	/// Return the `TileJSON` metadata view for this dataset.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Returns the parameters of the tiles reader.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Returns the coverage pyramid, computing it lazily on first access.
	///
	/// The first call scans the `tiles` table to derive the exact per-level
	/// coverage; the result is cached in [`TileSourceMetadata`] and reused by
	/// subsequent calls.
	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.metadata.get_or_compute_tile_pyramid(|| self.bbox_pyramid())
	}

	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
		// Collect all SQLite data synchronously (Connection is not Send)
		let tile_count: i64;
		let total_size: i64;
		let zoom_levels: String;
		let entries: Vec<(String, String)>;
		{
			let conn = self.pool.get()?;
			tile_count = conn.query_row("SELECT COUNT(*) FROM tiles", [], |row| row.get(0))?;
			total_size = conn.query_row("SELECT COALESCE(SUM(LENGTH(tile_data)), 0) FROM tiles", [], |row| {
				row.get(0)
			})?;
			zoom_levels = {
				let mut stmt = conn.prepare("SELECT DISTINCT zoom_level FROM tiles ORDER BY zoom_level")?;
				let levels: Vec<String> = stmt
					.query_map([], |row| row.get::<_, i32>(0))?
					.filter_map(std::result::Result::ok)
					.map(|z| z.to_string())
					.collect();
				levels.join(", ")
			};
			entries = {
				let mut stmt = conn.prepare("SELECT name, value FROM metadata ORDER BY name")?;
				stmt
					.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
					.filter_map(std::result::Result::ok)
					.collect()
			};
		}

		print.add_key_value("tile count", &tile_count).await;
		print.add_key_value("total data size", &total_size).await;
		print.add_key_value("zoom levels", &zoom_levels).await;

		if !entries.is_empty() {
			let p = print.get_list("metadata").await;
			for (name, value) in &entries {
				let display_value = if value.len() > 100 {
					format!("{}...", &value[..100])
				} else {
					value.clone()
				};
				p.add_key_value(name, &display_value).await;
			}
		}

		Ok(())
	}

	/// Fetch a single tile by XYZ coordinate.
	///
	/// Coordinates are converted to TMS row indexing internally (via `y' = 2^z - 1 - y`).
	/// Returns `Ok(None)` when the tile is not present.
	///
	/// # Errors
	/// Returns an error if the query fails.
	#[context("fetching tile {:?} from '{}'", coord, self.name)]
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		log::trace!("read tile from coord {coord:?}");

		let conn = self.pool.get()?;
		let mut stmt =
			conn.prepare("SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?")?;

		let max_index = 2u32.pow(u32::from(coord.level)) - 1;
		if let Ok(vec) = stmt.query_row([coord.x, max_index - coord.y, u32::from(coord.level)], |row| {
			row.get::<_, Vec<u8>>(0)
		}) {
			Ok(Some(Tile::from_blob(
				Blob::from(vec),
				*self.metadata.tile_compression(),
				*self.metadata.tile_format(),
			)))
		} else {
			Ok(None)
		}
	}

	#[context("streaming tile coords for bbox {:?}", bbox)]
	async fn tile_coord_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		bbox.flip_y();

		let conn = self.pool.get().unwrap();
		let mut stmt = conn
			.prepare(
				"SELECT tile_column, tile_row, zoom_level FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
			)
			.unwrap();

		let vec: Vec<(TileCoord, ())> = stmt
			.query_map(
				[
					bbox.x_min()?,
					bbox.x_max()?,
					bbox.y_min()?,
					bbox.y_max()?,
					u32::from(bbox.level()),
				],
				move |row| {
					let x = row.get::<_, u32>(0)?;
					let y = row.get::<_, u32>(1)?;
					let level = row.get::<_, u8>(2)?;
					let mut coord = TileCoord::new(level, x, y).unwrap();
					coord.flip_y();
					Ok((coord, ()))
				},
			)
			.unwrap()
			.filter_map(std::result::Result::ok)
			.collect();

		Ok(TileStream::from_vec(vec))
	}

	#[context("streaming tile sizes for bbox {:?}", bbox)]
	async fn tile_size_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		bbox.flip_y();

		let conn = self.pool.get().unwrap();
		let mut stmt = conn
			.prepare(
				"SELECT tile_column, tile_row, zoom_level, LENGTH(tile_data) FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
			)
			.unwrap();

		let vec: Vec<(TileCoord, u32)> = stmt
			.query_map(
				[
					bbox.x_min()?,
					bbox.x_max()?,
					bbox.y_min()?,
					bbox.y_max()?,
					u32::from(bbox.level()),
				],
				move |row| {
					let x = row.get::<_, u32>(0)?;
					let y = row.get::<_, u32>(1)?;
					let level = row.get::<_, u8>(2)?;
					let mut coord = TileCoord::new(level, x, y).unwrap();
					coord.flip_y();
					let size = row.get::<_, u32>(3)?;
					Ok((coord, size))
				},
			)
			.unwrap()
			.filter_map(std::result::Result::ok)
			.collect();

		Ok(TileStream::from_vec(vec))
	}

	/// Stream tiles within a single-zoom bounding box.
	///
	/// The input bbox is XYZ; rows are flipped to TMS for the query and flipped back on output.
	/// Empty bboxes yield an empty stream.
	///
	/// # Errors
	/// Returns an error if the query fails.
	#[context("streaming tiles for bbox {:?}", bbox)]
	async fn tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("mbtiles::tile_stream {bbox:?}");

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
					u32::from(bbox.level()),
				],
				move |row| {
					let x = row.get::<_, u32>(0)?;
					let y = row.get::<_, u32>(1)?;
					let level = row.get::<_, u8>(2)?;
					let mut coord = TileCoord::new(level, x, y).unwrap();
					coord.flip_y();
					let blob = Blob::from(row.get::<_, Vec<u8>>(3)?);
					let tile = Tile::from_blob(blob, *self.metadata.tile_compression(), *self.metadata.tile_format());
					Ok((coord, tile))
				},
			)
			.unwrap()
			.filter_map(std::result::Result::ok)
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

/// A struct representing a metadata record in the `MBTiles` database.
struct RecordMetadata {
	name: String,
	value: String,
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::MockWriter;
	use std::{env, path::PathBuf, sync::LazyLock};

	static PATH: LazyLock<PathBuf> = LazyLock::new(|| env::current_dir().unwrap().join("../testdata/berlin.mbtiles"));

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = MBTilesReader::open(&PATH, TilesRuntime::default())?;

		assert_eq!(
			format!("{reader:?}"),
			"MBTilesReader { parameters: TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full), tile_pyramid: RwLock { data: None, poisoned: false, .. } } }"
		);
		assert_eq!(
			reader.source_type().to_string(),
			format!("container 'mbtiles' ('{}')", PATH.to_str().unwrap())
		);
		assert_eq!(
			reader.tilejson().stringify(),
			"{\"author\":\"OpenStreetMap contributors, Geofabrik GmbH\",\"bounds\":[13.08283,52.33446,13.762245,52.6783],\"center\":[13.422538,52.50638,7],\"description\":\"Tile config for simple vector tiles schema\",\"license\":\"Open Database License 1.0\",\"maxzoom\":14,\"minzoom\":0,\"name\":\"Tilemaker to Geofabrik Vector Tiles schema\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"dummy\":\"Number\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":7},{\"fields\":{},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"kind\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"3.0\"}"
		);
		assert_eq!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full), tile_pyramid: RwLock { data: None, poisoned: false, .. } }"
		);
		assert_eq!(reader.metadata().tile_compression(), &Gzip);
		assert_eq!(reader.metadata().tile_format(), &MVT);

		let tile = reader
			.tile(&TileCoord::new(14, 8803, 5376)?)
			.await?
			.unwrap()
			.into_blob(reader.metadata().tile_compression())?;
		assert_eq!(tile.len(), 172969);
		assert_eq!(tile.range(0..10), &[31, 139, 8, 0, 0, 0, 0, 0, 0, 3]);
		assert_eq!(tile.range(172959..172969), &[255, 15, 172, 89, 205, 237, 7, 134, 5, 0]);

		MockWriter::write(&mut reader).await?;

		Ok(())
	}

	// Test tile fetching
	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		use versatiles_core::utils::PrettyPrint;

		let runtime = TilesRuntime::default();
		let reader = MBTilesReader::open(&PATH, runtime.clone())?;

		let mut printer = PrettyPrint::new();
		reader
			.probe_container(&mut printer.category("container").await, &runtime)
			.await?;
		assert_eq!(
			printer.stringify().await.split('\n').collect::<Vec<_>>(),
			[
				"container:",
				"  tile count: 878",
				"  total data size: 25_869_046",
				"  zoom levels: \"0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14\"",
				"  metadata:",
				"    author: \"OpenStreetMap contributors, Geofabrik GmbH\"",
				"    bounds: \"13.082830,52.334460,13.762245,52.678300\"",
				"    center: \"13.422538,52.506380,7\"",
				"    description: \"Tile config for simple vector tiles schema\"",
				"    format: \"pbf\"",
				"    json: \"{\\\"vector_layers\\\":[{\\\"id\\\":\\\"place_labels\\\",\\\"fields\\\":{\\\"kind\\\":\\\"String\\\",\\\"name\\\":\\\"String\\\",\\\"name_de\\\":\\\"String\\\",...\"",
				"    license: \"Open Database License 1.0\"",
				"    maxzoom: \"14\"",
				"    minzoom: \"0\"",
				"    name: \"Tilemaker to Geofabrik Vector Tiles schema\"",
				"    type: \"baselayer\"",
				"    version: \"3.0\"",
				""
			]
		);

		let mut printer = PrettyPrint::new();
		reader
			.probe_tiles(&mut printer.category("tiles").await, &runtime)
			.await?;

		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_matches_individual_reads() -> Result<()> {
		let reader = MBTilesReader::open(&PATH, TilesRuntime::default())?;

		// Use level 9 bbox which has 2x2 = 4 tiles according to the metadata
		let bbox = TileBBox::from_min_and_max(9, 274, 167, 275, 168)?;

		// Get all tiles via stream
		let stream = reader.tile_stream(bbox).await?;
		let stream_tiles: Vec<_> = stream.to_vec().await;
		assert_eq!(stream_tiles.len(), 4);

		// Verify each streamed tile matches individual read
		for (coord, mut tile) in stream_tiles {
			let stream_blob = tile.as_blob(reader.metadata().tile_compression())?;
			let single_blob = reader
				.tile(&coord)
				.await?
				.expect("tile should exist")
				.into_blob(reader.metadata().tile_compression())?;
			assert_eq!(
				stream_blob.as_slice(),
				single_blob.as_slice(),
				"blob mismatch at {coord:?}"
			);
		}

		Ok(())
	}
}
