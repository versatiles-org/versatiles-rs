use std::io::Read;

use super::container::TileType;
use crate::container::container::{self, TileCompression};

pub struct Reader {
	connection: rusqlite::Connection,
	minimum_level: Option<u64>,
	maximum_level: Option<u64>,
	tile_type: Option<TileType>,
	tile_compression: Option<TileCompression>,
	meta_data: Option<String>,
}
impl Reader {
	fn new(connection: rusqlite::Connection) -> Reader {
		Reader {
			connection,
			minimum_level: None,
			maximum_level: None,
			tile_type: None,
			tile_compression: None,
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
				"minzoom" => self.minimum_level = Some(val.parse::<u64>().unwrap()),
				"maxzoom" => self.maximum_level = Some(val.parse::<u64>().unwrap()),
				"format" => match val.as_str() {
					"jpg" => {
						self.tile_type = Some(TileType::JPG);
						self.tile_compression = Some(TileCompression::None);
					}
					"pbf" => {
						self.tile_type = Some(TileType::PBF);
						self.tile_compression = Some(TileCompression::Gzip);
					}
					"png" => {
						self.tile_type = Some(TileType::PNG);
						self.tile_compression = Some(TileCompression::None);
					}
					"webp" => {
						self.tile_type = Some(TileType::WEBP);
						self.tile_compression = Some(TileCompression::None);
					}
					_ => panic!("unknown format"),
				},
				"json" => self.meta_data = Some(val),
				&_ => {}
			}
		}

		if self.minimum_level.is_none() {
			panic!("'minzoom' is not defined in table 'metadata'");
		}
		if self.maximum_level.is_none() {
			panic!("'maxzoom' is not defined in table 'metadata'");
		}
		if self.tile_type.is_none() {
			panic!("'format' is not defined in table 'metadata'");
		}
		if self.meta_data.is_none() {
			panic!("'json' is not defined in table 'metadata'");
		}

		return Ok(());
	}
	fn calc_min_max(&self, level: u64, fun: &str, var: &str) -> rusqlite::Result<u64> {
		let sql = format!(
			"SELECT {}(tile_{}) FROM tiles WHERE zoom_level = {}",
			fun, var, level
		);
		let mut stmt = self.connection.prepare(sql.as_str())?;
		let row = stmt.query_row([], |entry| entry.get::<_, u64>(0))?;
		return Ok(row);
	}
}

impl container::Reader for Reader {
	fn load(filename: &std::path::PathBuf) -> std::io::Result<Box<dyn container::Reader>> {
		let reader = Self::load_sqlite(filename).expect("SQLite error");
		return Ok(Box::new(reader));
	}
	fn get_tile_type(&self) -> TileType {
		return self.tile_type.clone().unwrap();
	}
	fn get_tile_compression(&self) -> TileCompression {
		return self.tile_compression.clone().unwrap();
	}
	fn get_meta(&self) -> &[u8] {
		return self.meta_data.as_ref().unwrap().as_bytes();
	}
	fn get_minimum_level(&self) -> u64 {
		return self.minimum_level.unwrap();
	}
	fn get_maximum_level(&self) -> u64 {
		return self.maximum_level.unwrap();
	}
	fn get_minimum_col(&self, level: u64) -> u64 {
		return self.calc_min_max(level, "min", "column").unwrap();
	}
	fn get_maximum_col(&self, level: u64) -> u64 {
		return self.calc_min_max(level, "max", "column").unwrap();
	}
	fn get_minimum_row(&self, level: u64) -> u64 {
		return self.calc_min_max(level, "min", "row").unwrap();
	}
	fn get_maximum_row(&self, level: u64) -> u64 {
		return self.calc_min_max(level, "max", "row").unwrap();
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Result<Vec<u8>, &str> {
		let mut stmt = self
			.connection
			.prepare(
				"SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?",
			)
			.expect("SQL preparation failed");
		let data = stmt
			.query_row([level, col, row], |entry| entry.get::<_, Vec<u8>>(0))
			.expect("SQL query failes");
		return Ok(data);
	}
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Result<Vec<u8>, &str> {
		let data = self.get_tile_raw(level, col, row)?;
		return match self.tile_compression {
			Some(TileCompression::None) => Ok(data),
			Some(TileCompression::Gzip) => {
				let mut result: Vec<u8> = Vec::new();
				//println!("{:X?}", data);
				let _bytes_written = flate2::bufread::GzDecoder::new(data.as_slice())
					.read_to_end(&mut result)
					.unwrap();
				Ok(result)
			}
			Some(TileCompression::Brotli) => panic!("brotli decompression not implemented"),
			None => panic!(""),
		};
	}
}

pub struct Converter;
impl container::Converter for Converter {}
