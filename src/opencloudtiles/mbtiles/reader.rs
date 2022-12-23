use crate::opencloudtiles::{
	abstract_classes::{self, TileReaderParameters},
	types::{TileBBox, TileFormat},
};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OpenFlags;
use std::thread;

pub struct TileReader {
	pool: r2d2::Pool<SqliteConnectionManager>,
	meta_data: Option<String>,
	parameters: Option<TileReaderParameters>,
}
impl TileReader {
	fn new(pool: r2d2::Pool<SqliteConnectionManager>) -> TileReader {
		TileReader {
			pool,
			meta_data: None,
			parameters: None,
		}
	}
	fn load_from_sqlite(filename: &std::path::PathBuf) -> rusqlite::Result<TileReader> {
		let concurrency = thread::available_parallelism().unwrap().get();

		let manager = r2d2_sqlite::SqliteConnectionManager::file(filename)
			.with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI);

		let pool = r2d2::Pool::builder()
			.max_size(concurrency as u32)
			.build(manager)
			.unwrap();

		let mut reader = TileReader::new(pool);
		reader.load_meta_data()?;

		return Ok(reader);
	}
	fn load_meta_data(&mut self) -> rusqlite::Result<()> {
		let connection = self.pool.get().unwrap();
		let mut stmt = connection.prepare("SELECT name, value FROM metadata")?;
		let mut rows = stmt.query([])?;

		let mut min_zoom: Option<u64> = None;
		let mut max_zoom: Option<u64> = None;
		let mut tile_format: Option<TileFormat> = None;

		while let Some(row) = rows.next()? {
			let key = row.get::<_, String>(0)?;
			let val = row.get::<_, String>(1)?;
			//println!("name: {}, value: {}", key, val);
			match key.as_str() {
				"minzoom" => min_zoom = Some(val.parse::<u64>().unwrap()),
				"maxzoom" => max_zoom = Some(val.parse::<u64>().unwrap()),
				"format" => match val.as_str() {
					"jpg" => tile_format = Some(TileFormat::JPG),
					"pbf" => tile_format = Some(TileFormat::PBFGzip),
					"png" => tile_format = Some(TileFormat::PNG),
					"webp" => tile_format = Some(TileFormat::WEBP),
					_ => panic!("unknown format"),
				},
				"json" => self.meta_data = Some(val),
				&_ => {}
			}
		}

		self.parameters = Some(TileReaderParameters::new(
			min_zoom.unwrap(),
			max_zoom.unwrap(),
			tile_format.unwrap(),
			self.get_level_bboxes(),
		));

		if self.meta_data.is_none() {
			panic!("'json' is not defined in table 'metadata'");
		}

		return Ok(());
	}
	fn get_level_bboxes(&self) -> Vec<TileBBox> {
		let mut level_bboxes = Vec::new();

		let sql = "SELECT min(tile_row), max(tile_row), min(tile_column), max(tile_column),zoom_level FROM tiles GROUP BY zoom_level";
		let connection = self.pool.get().unwrap();
		let mut stmt = connection.prepare(sql).unwrap();

		let mut entries = stmt.query([]).unwrap();
		while let Some(entry) = entries.next().unwrap() {
			let row_min = entry.get_unwrap::<_, u64>("min(tile_row)");
			let row_max = entry.get_unwrap::<_, u64>("max(tile_row)");
			let col_min = entry.get_unwrap::<_, u64>("min(tile_column)");
			let col_max = entry.get_unwrap::<_, u64>("max(tile_column)");
			let level = entry.get_unwrap::<_, usize>("zoom_level");

			level_bboxes.insert(level, TileBBox::new(row_min, row_max, col_min, col_max));
		}

		return level_bboxes;
	}
}

impl abstract_classes::TileReader for TileReader {
	fn load(filename: &std::path::PathBuf) -> Result<Box<dyn abstract_classes::TileReader>, &str> {
		let reader = Self::load_from_sqlite(filename).expect("SQLite error");
		return Ok(Box::new(reader));
	}
	fn get_meta(&self) -> &[u8] {
		return self.meta_data.as_ref().unwrap().as_bytes();
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return self.parameters.as_ref().unwrap();
	}
	fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		let connection = self.pool.get().unwrap();
		let mut stmt = connection
			.prepare(
				"SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?",
			)
			.expect("SQL preparation failed");
		let result = stmt.query_row([level, col, row], |entry| entry.get::<_, Vec<u8>>(0));
		if result.is_ok() {
			return Some(result.unwrap());
		} else {
			return None;
		};
	}
}
