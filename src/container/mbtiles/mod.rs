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
}

pub struct Converter;
impl container::Converter for Converter {}
