use crate::opencloudtiles::{
	containers::abstract_container::{self, TileReaderBox, TileReaderTrait},
	types::{
		Blob, Precompression, TileBBox, TileBBoxPyramide, TileCoord3, TileFormat,
		TileReaderParameters,
	},
};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OpenFlags;
use std::{
	fmt::Debug,
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

		return reader;
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

		let sql = "SELECT zoom_level, min(tile_column), min(tile_row), max(tile_column), max(tile_row) FROM tiles GROUP BY zoom_level";
		let connection = self.pool.get().unwrap();
		let mut stmt = connection.prepare(sql).unwrap();

		let mut entries = stmt.query([]).unwrap();
		while let Some(entry) = entries.next().unwrap() {
			bbox_pyramide.set_level_bbox(
				entry.get_unwrap::<_, u64>("zoom_level"),
				TileBBox::new(
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

impl abstract_container::TileReaderTrait for TileReader {
	fn new(filename: &str) -> TileReaderBox {
		let path = Path::new(filename);
		if !path.exists() {
			panic!("file {} does not exists", filename)
		}

		let reader = Self::load_from_sqlite(&path.to_path_buf());
		return Box::new(reader);
	}
	fn get_meta(&self) -> Blob {
		return Blob::from_slice(self.meta_data.as_ref().unwrap().as_bytes());
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		return self.parameters.as_ref().unwrap();
	}
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob> {
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
			return Some(Blob::from_vec(result.unwrap()));
		} else {
			return None;
		};
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:MBTiles")
			.field("meta", &from_utf8(self.get_meta().as_slice()).unwrap())
			.field("parameters", &self.get_parameters())
			.finish()
	}
}
