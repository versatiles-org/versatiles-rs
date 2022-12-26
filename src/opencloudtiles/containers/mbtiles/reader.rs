use crate::opencloudtiles::{
	containers::abstract_container,
	types::{TileBBox, TileBBoxPyramide, TileCoord3, TileFormat, TileReaderParameters},
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
	pub fn new(pool: r2d2::Pool<SqliteConnectionManager>) -> TileReader {
		TileReader {
			pool,
			meta_data: None,
			parameters: None,
		}
	}
	fn load_from_sqlite(filename: &std::path::PathBuf) -> TileReader {
		let concurrency = thread::available_parallelism().unwrap().get();

		let manager = r2d2_sqlite::SqliteConnectionManager::file(filename)
			.with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI);

		let pool = r2d2::Pool::builder()
			.max_size(concurrency as u32)
			.build(manager)
			.unwrap();

		let mut reader = TileReader::new(pool);
		reader.load_meta_data();

		return reader;
	}
	fn load_meta_data(&mut self) {
		let connection = self.pool.get().unwrap();
		let mut stmt = connection
			.prepare("SELECT name, value FROM metadata")
			.expect("can not prepare SQL query");
		let mut entries = stmt.query([]).expect("SQL query failed");

		let mut tile_format: Option<TileFormat> = None;

		while let Some(entry) = entries.next().unwrap() {
			let key = entry.get::<_, String>(0).unwrap();
			let val = entry.get::<_, String>(1).unwrap();
			//println!("name: {}, value: {}", key, val);
			match key.as_str() {
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
			tile_format.unwrap(),
			self.get_bbox_pyramide(),
		));

		if self.meta_data.is_none() {
			panic!("'json' is not defined in table 'metadata'");
		}
	}
	fn get_bbox_pyramide(&self) -> TileBBoxPyramide {
		let mut bbox_pyramide = TileBBoxPyramide::new_empty();

		let sql = "SELECT zoom_level, min(tile_column), min(tile_row), max(tile_column), max(tile_row) FROM tiles GROUP BY zoom_level";
		let connection = self.pool.get().unwrap();
		let mut stmt = connection.prepare(sql).unwrap();

		let mut entries = stmt.query([]).unwrap();
		while let Some(entry) = entries.next().unwrap() {
			bbox_pyramide.set_level_bbox(
				entry.get_unwrap::<_, u64>("zoom_level"),
				&TileBBox::new(
					entry.get_unwrap::<_, u64>("min(tile_column)"),
					entry.get_unwrap::<_, u64>("min(tile_row)"),
					entry.get_unwrap::<_, u64>("max(tile_column)"),
					entry.get_unwrap::<_, u64>("max(tile_row)"),
				),
			);
		}

		return bbox_pyramide;
	}
}

impl abstract_container::TileReader for TileReader {
	fn load(filename: &std::path::PathBuf) -> Box<dyn abstract_container::TileReader> {
		let reader = Self::load_from_sqlite(filename);
		return Box::new(reader);
	}
	fn get_meta(&self) -> &[u8] {
		return self.meta_data.as_ref().unwrap().as_bytes();
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return self.parameters.as_ref().unwrap();
	}
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<Vec<u8>> {
		let connection = self.pool.get().unwrap();
		let mut stmt = connection
			.prepare(
				"SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?",
			)
			.expect("SQL preparation failed");
		let result = stmt.query_row([coord.z, coord.x, coord.y], |entry| {
			entry.get::<_, Vec<u8>>(0)
		});
		if result.is_ok() {
			return Some(result.unwrap());
		} else {
			return None;
		};
	}
}
