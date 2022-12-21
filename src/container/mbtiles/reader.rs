use std::io::Read;

use flate2::bufread::GzDecoder;

use crate::container::abstract_classes::{self, TileCompression, TileFormat};

pub struct Reader {
	connection: rusqlite::Connection,
	minimum_zoom: Option<u64>,
	maximum_zoom: Option<u64>,
	tile_format: Option<TileFormat>,
	tile_compression: Option<TileCompression>,
	meta_data: Option<String>,
}
impl Reader {
	fn new(connection: rusqlite::Connection) -> Reader {
		Reader {
			connection,
			minimum_zoom: None,
			maximum_zoom: None,
			tile_format: None,
			tile_compression: Some(TileCompression::None),
			meta_data: None,
		}
	}
	fn load_sqlite(filename: &std::path::PathBuf) -> rusqlite::Result<Reader> {
		let connection = rusqlite::Connection::open(filename)?;
		let mut reader = Reader::new(connection);
		reader.load_meta_data()?;

		// tiles from tiles
		//CREATE VIEW tiles AS   SELECT map.zoom_level as zoom_level,    map.tile_column as tile_column,    map.tile_row as tile_row,    images.tile_data as tile_data   FROM map JOIN images ON map.tile_id = images.tile_id;

		return Ok(reader);
	}
	fn load_meta_data(&mut self) -> rusqlite::Result<()> {
		let mut stmt = self
			.connection
			.prepare("SELECT name, value FROM metadata")?;
		let mut rows = stmt.query([])?;

		while let Some(row) = rows.next()? {
			let key = row.get::<_, String>(0)?;
			let val = row.get::<_, String>(1)?;
			//println!("name: {}, value: {}", key, val);
			match key.as_str() {
				"minzoom" => self.minimum_zoom = Some(val.parse::<u64>().unwrap()),
				"maxzoom" => self.maximum_zoom = Some(val.parse::<u64>().unwrap()),
				"format" => match val.as_str() {
					"jpg" => {
						self.tile_format = Some(TileFormat::JPG);
					}
					"pbf" => {
						self.tile_format = Some(TileFormat::PBF);
						self.tile_compression = Some(TileCompression::Gzip);
					}
					"png" => {
						self.tile_format = Some(TileFormat::PNG);
					}
					"webp" => {
						self.tile_format = Some(TileFormat::WEBP);
					}
					_ => panic!("unknown format"),
				},
				"json" => self.meta_data = Some(val),
				&_ => {}
			}
		}

		if self.minimum_zoom.is_none() {
			panic!("'minzoom' is not defined in table 'metadata'");
		}
		if self.maximum_zoom.is_none() {
			panic!("'maxzoom' is not defined in table 'metadata'");
		}
		if self.tile_format.is_none() {
			panic!("'format' is not defined in table 'metadata'");
		}
		if self.meta_data.is_none() {
			panic!("'json' is not defined in table 'metadata'");
		}

		return Ok(());
	}
	fn calc_level_bbox(&self, level: u64) -> rusqlite::Result<(u64, u64, u64, u64)> {
		let sql = format!(
			"SELECT min(tile_row), max(tile_row), min(tile_column), max(tile_column) FROM tiles WHERE zoom_level = {}",
			level
		);
		let mut stmt = self.connection.prepare(sql.as_str())?;
		let row = stmt.query_row([], |entry| {
			Ok((
				entry.get::<_, u64>(0)?,
				entry.get::<_, u64>(1)?,
				entry.get::<_, u64>(2)?,
				entry.get::<_, u64>(3)?,
			))
		});
		return row;
	}
}

impl abstract_classes::Reader for Reader {
	fn load(filename: &std::path::PathBuf) -> std::io::Result<Box<dyn abstract_classes::Reader>> {
		let reader = Self::load_sqlite(filename).expect("SQLite error");
		return Ok(Box::new(reader));
	}
	fn get_tile_format(&self) -> TileFormat {
		return self.tile_format.clone().unwrap();
	}
	fn get_tile_compression(&self) -> TileCompression {
		return self.tile_compression.clone().unwrap();
	}
	fn get_meta(&self) -> &[u8] {
		return self.meta_data.as_ref().unwrap().as_bytes();
	}
	fn get_minimum_zoom(&self) -> u64 {
		return self.minimum_zoom.unwrap();
	}
	fn get_maximum_zoom(&self) -> u64 {
		return self.maximum_zoom.unwrap();
	}
	fn get_level_bbox(&self, level: u64) -> (u64, u64, u64, u64) {
		return self.calc_level_bbox(level).unwrap();
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		let mut stmt = self
			.connection
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
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		let data = self.get_tile_raw(level, col, row);
		if data.is_none() {
			return None;
		}
		let tile = data.unwrap();
		return match self.tile_compression {
			Some(TileCompression::None) => Some(tile),
			Some(TileCompression::Gzip) => {
				let mut result: Vec<u8> = Vec::new();
				//println!("{:X?}", tile);
				let _bytes_written = GzDecoder::new(tile.as_slice())
					.read_to_end(&mut result)
					.unwrap();
				Some(result)
			}
			Some(TileCompression::Brotli) => panic!("brotli decompression not implemented"),
			None => panic!(""),
		};
	}
}
