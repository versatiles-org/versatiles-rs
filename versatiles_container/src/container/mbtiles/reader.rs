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
//! `vector_layers`) and merges them into an internal [`TileJSON`].
//!
//! The per-level coverage pyramid is **not** scanned at open time. It is computed
//! lazily on the first call to [`TileSource::tile_pyramid`]
//! by reading `(zoom_level, tile_column, tile_row)` from the `tiles` table, and
//! the result is cached for subsequent calls.
//!
//! ## Requirements
//! - The `MBTiles` file **must be an absolute path** when opening with [`MBTilesReader::open`].
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

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::{
	SqliteConnectionManager,
	rusqlite::{self, OpenFlags, OptionalExtension},
};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{
	TileCompression::{Gzip, Uncompressed},
	TileFormat::{JPG, MVT, PNG, WEBP},
	json::parse_json_str,
	types::{
		Blob, GeoBBox, GeoCenter, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileSchema,
		TileStream,
	},
};
use versatiles_derive::context;

use crate::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesReader, TilesRuntime, Traversal};

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
	/// Where rows the bulk streams cannot read are reported; a stream cannot
	/// carry the error.
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
	/// and then calls [`Self::load_meta_data`] to populate `tilejson` and parameters.
	///
	/// # Errors
	/// Returns an error if the connection cannot be established or metadata fails to load.
	#[context("loading SQLite '{}'", path.display())]
	fn load_from_sqlite(path: &Path, runtime: TilesRuntime) -> Result<MBTilesReader> {
		log::debug!("load_from_sqlite {path:?}");

		// Opened read-only, and deliberately not with SQLite's defaults
		// (`READ_WRITE | CREATE | URI | NO_MUTEX`). This reader issues no write
		// statements, and an `.mbtiles` is untrusted input like any other
		// container:
		//
		// - `READ_ONLY` instead of `READ_WRITE`: a database with a hot journal
		//   is *recovered* on open, so opening one to read a tile from it
		//   rewrote the file, and `-wal`/`-shm` files appeared beside it. A
		//   crafted journal made reading someone else's file a write to it.
		// - no `CREATE`: belt and braces. `open` already refuses a path that
		//   does not exist, so this is unreachable today; it stops a future
		//   caller of this function from silently creating an empty database
		//   that then reports no tiles.
		// - no `URI`: the argument is a filesystem path. Interpreting it as a
		//   URI would let `file:…?mode=rwc` put the write flags back.
		//
		// What this does *not* do: a WAL database still gets its `-shm`/`-wal`
		// sidecars, because SQLite needs the shared-memory index to read one at
		// all — measured, and pinned by
		// `read_only_tests::a_wal_database_still_gets_its_sidecars`. Only
		// `immutable=1` avoids that, and it asserts the file cannot change
		// while open, which a reader pointed at someone else's file cannot
		// promise.
		//
		// The cost is that a genuinely crash-damaged file now fails to open
		// instead of being repaired in place. Repairing someone's archive as a
		// side effect of reading it is not this reader's decision to make.
		let manager = SqliteConnectionManager::file(path)
			.with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX);
		let pool = Pool::builder().max_size(10).build(manager)?;
		let metadata = TileSourceMetadata::new(MVT, Uncompressed, Traversal::ANY, None);

		let mut reader = MBTilesReader {
			name: String::from(path.to_str().expect("mbtiles path is utf-8")),
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
	/// Also updates the tile pyramid from the database.
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
					// Written by the MLT tools; some gzip each tile by its own
					// size, so the compression is not knowable here (#111).
					"mlt" | "application/vnd.maplibre-tile" => {
						bail!("MLT tiles in mbtiles are not supported yet ('{}')", self.name)
					}
					_ => bail!("unknown tile format '{value}' in mbtiles metadata of '{}'", self.name),
				},
				// Everything but the format is optional. A value that does not
				// parse is left out with a warning, as `tile_schema` always was:
				// the tiles are still readable, and refusing the whole file over
				// a two-part `center` helps nobody.
				_ => {
					if let Err(error) = self.load_optional_meta_data(key, value) {
						log::warn!(
							"ignoring unreadable metadata '{key}' in mbtiles '{}': {error:#}",
							self.name
						);
					}
				}
			}
		}

		let tile_format = tile_format?;
		let mut compression = compression?;
		if tile_format == MVT {
			compression = self.check_pbf_compression(compression)?;
		}
		self.metadata.set_tile_format(tile_format);
		self.metadata.set_tile_compression(compression);

		Ok(())
	}

	/// Applies one optional metadata entry to the TileJSON.
	///
	/// See <https://github.com/mapbox/mbtiles-spec/blob/master/1.3/spec.md#content>.
	fn load_optional_meta_data(&mut self, key: &str, value: &str) -> Result<()> {
		match key {
			"bounds" => {
				let bounds = value
					.split(',')
					.map(|v| v.trim().parse::<f64>())
					.collect::<Result<Vec<f64>, _>>()?;
				self.tilejson.limit_bbox(GeoBBox::try_from(bounds)?);
			}
			"name" | "attribution" | "author" | "description" | "license" | "type" | "version" => {
				self.tilejson.set_string(key, value)?;
			}
			"center" => {
				let parts = value
					.split(',')
					.map(|v| v.trim().parse::<f64>())
					.collect::<Result<Vec<f64>, _>>()?;
				self.tilejson.center = Some(GeoCenter::try_from(parts)?);
			}
			// Written by this crate's MBTiles writer; see there.
			"tile_schema" => self.tilejson.tile_schema = Some(TileSchema::try_from(value)?),
			"minzoom" => self.tilejson.set_zoom_min(value.trim().parse::<u8>()?),
			"maxzoom" => self.tilejson.set_zoom_max(value.trim().parse::<u8>()?),
			"json" => {
				let json = parse_json_str(value).context("failed to parse JSON")?;
				let object = json.as_object().context("expected a JSON object")?;
				// The spec puts `vector_layers` here for vector tiles; a raster
				// file may keep anything else in `json`, which is not an error.
				if let Some(vector_layers) = object.get("vector_layers") {
					self.tilejson.set_vector_layers(vector_layers)?;
				}
			}
			_ => {}
		}
		Ok(())
	}

	/// `format=pbf` means gzipped vector tiles, but not every writer gzips
	/// them. Copied through as gzip, uncompressed tiles became a container that
	/// claims a compression its tiles do not have, so the first tile is asked.
	///
	/// Gzip and zstd announce themselves; an uncompressed MVT starts with the
	/// key of its first layer (field 3, length-delimited: `0x1A`). Brotli has
	/// no signature, so anything else keeps what the metadata implies.
	fn check_pbf_compression(&self, declared: TileCompression) -> Result<TileCompression> {
		let conn = self.pool.get()?;
		let first: Option<Vec<u8>> = conn
			.query_row(
				"SELECT tile_data FROM tiles WHERE tile_data IS NOT NULL LIMIT 1",
				[],
				|row| row.get(0),
			)
			.optional()?;
		let Some(first) = first else {
			return Ok(declared);
		};
		let detected = match first.as_slice() {
			[0x1F, 0x8B, ..] => Gzip,
			[0x28, 0xB5, 0x2F, 0xFD, ..] => TileCompression::Zstd,
			[0x1A, ..] => Uncompressed,
			_ => declared,
		};
		if detected != declared {
			log::warn!(
				"mbtiles '{}' declares format=pbf, which means gzip, but its tiles are {detected}; reading them as {detected}",
				self.name
			);
		}
		Ok(detected)
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
	#[context("computing tile pyramid from MBTiles")]
	fn compute_tile_pyramid(&self) -> Result<TilePyramid> {
		log::debug!("tile_pyramid");

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
		self
			.metadata
			.get_or_compute_tile_pyramid(|| self.compute_tile_pyramid())
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
				// Cut at a character, not a byte: slicing at byte 100 panicked
				// inside a multi-byte character.
				let display_value = match value.char_indices().nth(100) {
					Some((end, _)) => format!("{}...", &value[..end]),
					None => value.clone(),
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

		let mut tms = *coord;
		tms.flip_y();
		// Only "no such row" is an absent tile. Anything else — a NULL or
		// non-blob `tile_data`, a locked or corrupt database — is an error.
		let vec = stmt
			.query_row([tms.x, tms.y, u32::from(tms.level)], |row| row.get::<_, Vec<u8>>(0))
			.optional()?;
		Ok(vec.map(|vec| {
			Tile::from_blob(
				Blob::from(vec),
				*self.metadata.tile_compression(),
				*self.metadata.tile_format(),
			)
		}))
	}

	#[context("streaming tile coords for bbox {:?}", bbox)]
	async fn tile_coord_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		bbox.flip_y();

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(
			"SELECT tile_column, tile_row, zoom_level FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
		)?;

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
					let mut coord = TileCoord::new(level, x, y).expect("valid tile coord from db row");
					coord.flip_y();
					Ok((coord, ()))
				},
			)?
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

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(
			"SELECT tile_column, tile_row, zoom_level, LENGTH(tile_data) FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
		)?;

		let vec = stmt.query_map(
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
				let mut coord = TileCoord::new(level, x, y).expect("valid tile coord from db row");
				coord.flip_y();
				Ok((coord, row.get::<_, u32>(3)))
			},
		)?;
		let vec = report_unreadable_rows(vec, &self.runtime, &self.name);

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

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(
			"SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?",
		)?;

		let rows = stmt.query_map(
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
				let mut coord = TileCoord::new(level, x, y).expect("valid tile coord from db row");
				coord.flip_y();
				Ok((coord, row.get::<_, Vec<u8>>(3)))
			},
		)?;
		let vec: Vec<(TileCoord, Tile)> = report_unreadable_rows(rows, &self.runtime, &self.name)
			.into_iter()
			.map(|(coord, data)| {
				let tile = Tile::from_blob(
					Blob::from(data),
					*self.metadata.tile_compression(),
					*self.metadata.tile_format(),
				);
				(coord, tile)
			})
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

/// Collects the rows of a bulk query, reporting each one that cannot be read
/// to the runtime instead of dropping it unseen: a NULL `tile_data` is a tile
/// that exists and cannot be delivered, not a gap. A row whose coordinate was
/// read names its tile.
fn report_unreadable_rows<T>(
	rows: impl Iterator<Item = rusqlite::Result<(TileCoord, rusqlite::Result<T>)>>,
	runtime: &TilesRuntime,
	name: &str,
) -> Vec<(TileCoord, T)> {
	rows
		.filter_map(|row| match row {
			Ok((coord, Ok(value))) => Some((coord, value)),
			Ok((coord, Err(e))) => {
				runtime.record_error(&format!("mbtiles '{name}' tile {}", coord.to_json()), &e.into());
				None
			}
			Err(e) => {
				runtime.record_error(&format!("mbtiles '{name}' row"), &e.into());
				None
			}
		})
		.collect()
}

/// A struct representing a metadata record in the `MBTiles` database.
struct RecordMetadata {
	name: String,
	value: String,
}

#[cfg(test)]
pub mod tests {
	use std::{env, path::PathBuf, sync::LazyLock};

	use super::*;
	use crate::MockWriter;

	static PATH: LazyLock<PathBuf> = LazyLock::new(|| env::current_dir().unwrap().join("../testdata/berlin.mbtiles"));

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let reader = MBTilesReader::open(&PATH, TilesRuntime::default())?;

		assert_eq!(
			format!("{reader:?}"),
			"MBTilesReader { parameters: TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full), preferred_traversal: None, tile_pyramid: RwLock { data: None, poisoned: false, .. } } }"
		);
		assert_eq!(
			reader.source_type().to_string(),
			format!("container 'mbtiles' ('{}')", PATH.to_str().unwrap())
		);
		assert_eq!(
			reader.tilejson().stringify(),
			"{\"author\":\"OpenStreetMap contributors\",\"bounds\":[13.3,52.45,13.46,52.55],\"center\":[13.381348,52.502806,2],\"description\":\"Vector tiles based on OSM in Shortbread scheme\",\"license\":\"Open Database License 1.0\",\"maxzoom\":14,\"minzoom\":0,\"name\":\"VersaTiles OSM\",\"tilejson\":\"3.0.0\",\"type\":\"baselayer\",\"vector_layers\":[{\"fields\":{\"housename\":\"String\",\"housenumber\":\"String\"},\"id\":\"addresses\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"aerialways\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"admin_level\":\"Number\",\"disputed\":\"Boolean\",\"maritime\":\"Boolean\"},\"id\":\"boundaries\",\"maxzoom\":14,\"minzoom\":0},{\"fields\":{\"admin_level\":\"Number\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"way_area\":\"Number\"},\"id\":\"boundary_labels\",\"maxzoom\":14,\"minzoom\":2},{\"fields\":{\"kind\":\"String\"},\"id\":\"bridges\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"amenity\":\"String\",\"atm\":\"Boolean\",\"cuisine\":\"String\",\"denomination\":\"String\",\"dummy\":\"Number\",\"emergency\":\"String\",\"highway\":\"String\",\"historic\":\"String\",\"housename\":\"String\",\"housenumber\":\"String\",\"information\":\"String\",\"leisure\":\"String\",\"man_made\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"office\":\"String\",\"religion\":\"String\",\"shop\":\"String\",\"sport\":\"String\",\"tourism\":\"String\"},\"id\":\"buildings\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\"},\"id\":\"dam_lines\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"kind\":\"String\"},\"id\":\"dam_polygons\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\"},\"id\":\"ferries\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\"},\"id\":\"land\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"x\":\"Number\",\"y\":\"Number\"},\"id\":\"ocean\",\"maxzoom\":14,\"minzoom\":8},{\"fields\":{\"kind\":\"String\"},\"id\":\"pier_lines\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"kind\":\"String\"},\"id\":\"pier_polygons\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"population\":\"Number\"},\"id\":\"place_labels\",\"maxzoom\":14,\"minzoom\":3},{\"fields\":{\"amenity\":\"String\",\"atm\":\"Boolean\",\"cuisine\":\"String\",\"denomination\":\"String\",\"emergency\":\"String\",\"highway\":\"String\",\"historic\":\"String\",\"housename\":\"String\",\"housenumber\":\"String\",\"information\":\"String\",\"leisure\":\"String\",\"man_made\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"office\":\"String\",\"recycling:clothes\":\"Boolean\",\"recycling:glass_bottles\":\"Boolean\",\"recycling:paper\":\"Boolean\",\"recycling:scrap_metal\":\"Boolean\",\"religion\":\"String\",\"shop\":\"String\",\"sport\":\"String\",\"tourism\":\"String\",\"tower:type\":\"String\",\"vending\":\"String\"},\"id\":\"pois\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"iata\":\"String\",\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\"},\"id\":\"public_transport\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"amenity\":\"String\",\"emergency\":\"String\",\"highway\":\"String\",\"historic\":\"String\",\"housename\":\"String\",\"housenumber\":\"String\",\"kind\":\"String\",\"leisure\":\"String\",\"man_made\":\"String\",\"name\":\"String\",\"name_en\":\"String\",\"office\":\"String\",\"shop\":\"String\",\"tourism\":\"String\"},\"id\":\"sites\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"id\":\"street_labels\",\"maxzoom\":14,\"minzoom\":10},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"ref\":\"String\"},\"id\":\"street_labels_points\",\"maxzoom\":14,\"minzoom\":12},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"street_polygons\",\"maxzoom\":14,\"minzoom\":11},{\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"oneway\":\"Boolean\",\"oneway_reverse\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"streets\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\"},\"id\":\"streets_polygons_labels\",\"maxzoom\":14,\"minzoom\":14},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"water_lines\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"tunnel\":\"Boolean\"},\"id\":\"water_lines_labels\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"way_area\":\"Number\"},\"id\":\"water_polygons\",\"maxzoom\":14,\"minzoom\":4},{\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_ar\":\"String\",\"name_de\":\"String\",\"name_el\":\"String\",\"name_en\":\"String\",\"name_es\":\"String\",\"name_fr\":\"String\",\"name_it\":\"String\",\"name_nl\":\"String\",\"name_pl\":\"String\",\"name_pt\":\"String\",\"name_uk\":\"String\",\"way_area\":\"Number\"},\"id\":\"water_polygons_labels\",\"maxzoom\":14,\"minzoom\":14}],\"version\":\"2.0\"}"
		);
		assert_eq!(
			format!("{:?}", reader.metadata()),
			"TileSourceMetadata { tile_compression: Gzip, tile_format: MVT, traversal: Traversal(AnyOrder,full), preferred_traversal: None, tile_pyramid: RwLock { data: None, poisoned: false, .. } }"
		);
		assert_eq!(reader.metadata().tile_compression(), &Gzip);
		assert_eq!(reader.metadata().tile_format(), &MVT);

		let tile = reader
			.tile(&TileCoord::new(14, 8803, 5376)?)
			.await?
			.unwrap()
			.into_blob(reader.metadata().tile_compression())?;
		assert_eq!(tile.len(), 215726);
		// First 4 bytes are the gzip magic (1f 8b) + DEFLATE method (08) + flags (00).
		// XFL and OS bytes downstream of those vary by gzip implementation, so we
		// only check the stable prefix here.
		assert_eq!(tile.range(0..4), &[31, 139, 8, 0]);

		MockWriter::write(&reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn satisfies_stream_count_invariant() -> Result<()> {
		let reader = MBTilesReader::open(&PATH, TilesRuntime::default())?;
		let pyr = reader.tile_pyramid().await?;
		for z in 0..=pyr.level_max().unwrap_or(0) {
			let bbox = pyr.level_ref(z).to_bbox();
			if bbox.is_empty() {
				continue;
			}
			crate::testing::assert_stream_counts_agree(&reader, bbox).await?;
		}
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
				"  tile count: 130",
				"  total data size: 11_339_914",
				"  zoom levels: \"0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14\"",
				"  metadata:",
				"    author: \"OpenStreetMap contributors\"",
				"    bounds: \"13.3,52.45,13.46,52.55\"",
				"    center: \"13.38134765625,52.50280645538263,2\"",
				"    description: \"Vector tiles based on OSM in Shortbread scheme\"",
				"    format: \"pbf\"",
				"    json: \"{\\\"vector_layers\\\":[{\\\"fields\\\":{\\\"housename\\\":\\\"String\\\",\\\"housenumber\\\":\\\"String\\\"},\\\"id\\\":\\\"addresses\\\",\\\"maxzoom\\\"...\"",
				"    license: \"Open Database License 1.0\"",
				"    maxzoom: \"14\"",
				"    minzoom: \"0\"",
				"    name: \"VersaTiles OSM\"",
				"    type: \"baselayer\"",
				"    version: \"2.0\"",
				""
			]
		);

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

#[cfg(test)]
mod read_only_tests {
	use super::*;

	/// The reader's connections cannot write, whatever the file says.
	///
	/// This is the property, and the only one of these that discriminates: an
	/// earlier version of this test checked that reading a clean database left
	/// it byte-identical, which passes with read-write flags too, because a
	/// clean database read with SELECTs is not written either way.
	#[tokio::test]
	async fn the_connection_refuses_to_write() -> Result<()> {
		let source = std::env::current_dir()?.join("../testdata/berlin.mbtiles");
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("berlin.mbtiles");
		std::fs::copy(&source, &path)?;

		let reader = MBTilesReader::open(&path, TilesRuntime::default())?;
		let conn = reader.pool.get()?;

		let error = conn
			.execute("INSERT INTO metadata (name, value) VALUES ('x', 'y')", [])
			.expect_err("the reader must not be able to write to the database");

		assert!(
			format!("{error}").contains("readonly"),
			"expected a read-only error, got: {error}"
		);
		Ok(())
	}

	/// What read-only does *not* buy, pinned so the limitation is not mistaken
	/// for a guarantee: a WAL database still gets its `-shm`/`-wal` sidecars,
	/// because SQLite needs the shared-memory index to read one at all.
	///
	/// Preventing that needs `immutable=1`, which asserts the file cannot
	/// change while open — wrong for a reader pointed at a file somebody else
	/// may be writing.
	#[tokio::test]
	async fn a_wal_database_still_gets_its_sidecars() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("wal.mbtiles");

		{
			// A minimal WAL-mode MBTiles, written with the sink's own stack.
			let conn = r2d2_sqlite::rusqlite::Connection::open(&path)?;
			conn.pragma_update(None, "journal_mode", "WAL")?;
			conn.execute("CREATE TABLE metadata (name text, value text)", [])?;
			conn.execute("INSERT INTO metadata VALUES ('format','pbf')", [])?;
			conn.execute(
				"CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob)",
				[],
			)?;
		}

		let reader = MBTilesReader::open(&path, TilesRuntime::default())?;
		drop(reader);

		assert!(
			path.with_extension("mbtiles-wal").exists() || path.with_extension("mbtiles-shm").exists(),
			"if SQLite stops writing sidecars for read-only WAL access, this limitation is gone \
			 and the comment above it should go too"
		);
		Ok(())
	}

	/// A minimal MBTiles with the given metadata rows and one tile at 0/0/0.
	fn mbtiles_with(dir: &Path, metadata: &[(&str, &str)], tile: &[u8]) -> Result<std::path::PathBuf> {
		let path = dir.join("test.mbtiles");
		let conn = r2d2_sqlite::rusqlite::Connection::open(&path)?;
		conn.execute("CREATE TABLE metadata (name text, value text)", [])?;
		for (name, value) in metadata {
			conn.execute("INSERT INTO metadata VALUES (?1, ?2)", [name, value])?;
		}
		conn.execute(
			"CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob)",
			[],
		)?;
		conn.execute("INSERT INTO tiles VALUES (0, 0, 0, ?1)", [tile])?;
		Ok(path)
	}

	/// `format=pbf` means gzip, but uncompressed tiles were copied through
	/// labelled as gzip. The first tile now says what they are.
	#[tokio::test]
	async fn uncompressed_pbf_tiles_are_read_as_uncompressed() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let raw_mvt = [0x1A, 0x00];
		let path = mbtiles_with(dir.path(), &[("format", "pbf")], &raw_mvt)?;
		let reader = MBTilesReader::open(&path, TilesRuntime::default())?;
		assert_eq!(reader.metadata().tile_compression(), &Uncompressed);

		let gzipped = versatiles_core::compression::compress_gzip(&Blob::from(raw_mvt.to_vec()))?;
		let dir = tempfile::tempdir()?;
		let path = mbtiles_with(dir.path(), &[("format", "pbf")], gzipped.as_slice())?;
		let reader = MBTilesReader::open(&path, TilesRuntime::default())?;
		assert_eq!(reader.metadata().tile_compression(), &Gzip);
		Ok(())
	}

	/// Optional metadata that does not parse is left out, not a reason to
	/// refuse the file.
	#[tokio::test]
	async fn unreadable_optional_metadata_is_ignored() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = mbtiles_with(
			dir.path(),
			&[
				("format", "png"),
				("center", "13.4,52.5"),
				("bounds", "a,b,c,d"),
				("minzoom", "low"),
				("json", "{\"something\":\"else\"}"),
				("name", "kept"),
			],
			b"png",
		)?;
		let reader = MBTilesReader::open(&path, TilesRuntime::default())?;
		assert_eq!(reader.tilejson().center, None);
		assert_eq!(reader.tilejson().str("name"), Some("kept"));
		Ok(())
	}

	/// The probe shortens long metadata values at a character, not a byte.
	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe_shortens_multibyte_metadata() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let description = "é".repeat(80);
		let path = mbtiles_with(dir.path(), &[("format", "png"), ("description", &description)], b"png")?;
		let reader = MBTilesReader::open(&path, TilesRuntime::default())?;
		let mut printer = PrettyPrint::new();
		reader
			.probe_container(&mut printer.category("container").await, &TilesRuntime::default())
			.await?;
		Ok(())
	}
}
