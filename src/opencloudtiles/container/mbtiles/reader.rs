use crate::opencloudtiles::{
	container::{TileReaderBox, TileReaderTrait},
	lib::*,
};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OpenFlags;
use std::{
	env::current_dir,
	path::{Path, PathBuf},
	str::from_utf8,
	thread,
};

pub struct TileReader {
	name: String,
	pool: r2d2::Pool<SqliteConnectionManager>,
	meta_data: Option<String>,
	parameters: Option<TileReaderParameters>,
}
impl TileReader {
	fn load_from_sqlite(filename: &PathBuf) -> TileReader {
		let concurrency = thread::available_parallelism().unwrap().get();

		let manager = r2d2_sqlite::SqliteConnectionManager::file(filename)
			.with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI);

		let pool = r2d2::Pool::builder()
			.max_size(concurrency as u32)
			.build(manager)
			.unwrap();

		let mut reader = TileReader {
			name: filename.to_string_lossy().to_string(),
			pool,
			meta_data: None,
			parameters: None,
		};
		reader.load_meta_data();

		reader
	}
	fn load_meta_data(&mut self) {
		let connection = self.pool.get().unwrap();
		let mut stmt = connection
			.prepare("SELECT name, value FROM metadata")
			.expect("can not prepare SQL query");
		let mut entries = stmt.query([]).expect("SQL query failed");

		let mut tile_format: Option<TileFormat> = None;
		let mut precompression: Option<Precompression> = None;

		while let Some(entry) = entries.next().unwrap() {
			let key = entry.get::<_, String>(0).unwrap();
			let val = entry.get::<_, String>(1).unwrap();

			//println!("name: {}, value: {}", key, val);

			match key.as_str() {
				"format" => match val.as_str() {
					"jpg" => {
						tile_format = Some(TileFormat::JPG);
						precompression = Some(Precompression::Uncompressed);
					}
					"pbf" => {
						tile_format = Some(TileFormat::PBF);
						precompression = Some(Precompression::Gzip);
					}
					"png" => {
						tile_format = Some(TileFormat::PNG);
						precompression = Some(Precompression::Uncompressed);
					}
					"webp" => {
						tile_format = Some(TileFormat::WEBP);
						precompression = Some(Precompression::Uncompressed);
					}
					_ => panic!("unknown format"),
				},
				"json" => self.meta_data = Some(val),
				&_ => {}
			}
		}

		self.parameters = Some(TileReaderParameters::new(
			tile_format.unwrap(),
			precompression.unwrap(),
			self.get_bbox_pyramide(),
		));

		if self.meta_data.is_none() {
			panic!("'json' is not defined in table 'metadata'");
		}
	}
	fn get_bbox_pyramide(&self) -> TileBBoxPyramide {
		let mut bbox_pyramide = TileBBoxPyramide::new_empty();
		let connection = self.pool.get().unwrap();

		let query = |sql1: &str, sql2: &str| -> u64 {
			let sql = if sql2.is_empty() {
				format!("SELECT {} FROM tiles", sql1)
			} else {
				format!("SELECT {} FROM tiles WHERE {}", sql1, sql2)
			};

			connection.query_row(&sql, [], |r| r.get(0)).unwrap()
		};

		let z0 = query("MIN(zoom_level)", "");
		let z1 = query("MAX(zoom_level)", "");

		for z in z0..=z1 {
			let x0 = query("MIN(tile_column)", &format!("zoom_level = {}", z));
			let x1 = query("MAX(tile_column)", &format!("zoom_level = {}", z));
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
				> SELECT MIN(tile_row) FROM tiles WHERE zoom_level = 14 AND tile_column = $center_column
				This takes only a few milliseconds.

				The second query calculates MIN(tile_row) for all columns, but starting with the estimate:
				> SELECT MIN(tile_row) FROM tiles WHERE zoom_level = 14 AND tile_row <= $min_row_estimate

				This seems to be a great help. I suspect it helps SQLite so it doesn't have to scan the entire index/table.
			*/

			let sql_prefix = format!("zoom_level = {} AND tile_", z);
			let mut y0 = query("MIN(tile_row)", &format!("{}column = {}", sql_prefix, xc));
			let mut y1 = query("MAX(tile_row)", &format!("{}column = {}", sql_prefix, xc));

			y0 = query("MIN(tile_row)", &format!("{}row <= {}", sql_prefix, y0));
			y1 = query("MAX(tile_row)", &format!("{}row >= {}", sql_prefix, y1));

			let max_y = 2u64.pow(z as u32) - 1;

			bbox_pyramide.set_level_bbox(z, TileBBox::new(x0, max_y - y1, x1, max_y - y0));
		}

		bbox_pyramide
	}
}

impl TileReaderTrait for TileReader {
	fn new(path: &str) -> TileReaderBox {
		let mut filename = current_dir().unwrap();
		filename.push(Path::new(path));

		assert!(filename.exists(), "file {:?} does not exist", filename);
		assert!(
			filename.is_absolute(),
			"path {:?} must be absolute",
			filename
		);

		filename = filename.canonicalize().unwrap();

		Box::new(Self::load_from_sqlite(&filename))
	}
	fn get_meta(&self) -> Blob {
		Blob::from_slice(self.meta_data.as_ref().unwrap().as_bytes())
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		self.parameters.as_ref().unwrap()
	}
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob> {
		let connection = self.pool.get().unwrap();
		let mut stmt = connection
			.prepare(
				"SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?",
			)
			.expect("SQL preparation failed");

		let max_index = 2u64.pow(coord.z as u32) - 1;
		let result = stmt.query_row([coord.x, max_index - coord.y, coord.z], |entry| {
			entry.get::<_, Vec<u8>>(0)
		});

		if let Ok(vec) = result {
			Some(Blob::from_vec(vec))
		} else {
			None
		}
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl std::fmt::Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:MBTiles")
			.field("meta", &from_utf8(self.get_meta().as_slice()).unwrap())
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
