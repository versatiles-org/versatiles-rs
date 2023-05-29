use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	shared::{
		Blob, Compression, Error, ProgressBar, Result, TileBBox, TileBBoxPyramid, TileCoord2, TileCoord3, TileFormat,
		TileReaderParameters,
	},
};
use async_trait::async_trait;
use futures::executor::block_on;
use log::trace;
use rusqlite::{Connection, OpenFlags};
use std::{
	env::current_dir,
	path::{Path, PathBuf},
	thread,
};
use tokio::sync::Mutex;

const MB: usize = 1024 * 1024;

pub struct TileReader {
	name: String,
	connection: Mutex<Connection>,
	meta_data: Option<String>,
	parameters: TileReaderParameters,
}
impl TileReader {
	async fn load_from_sqlite(filename: &PathBuf) -> Result<TileReader> {
		trace!("load_from_sqlite {:?}", filename);

		let concurrency = thread::available_parallelism()?.get();

		let connection = Connection::open_with_flags(filename, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

		connection.pragma_update(None, "mmap_size", 256 * MB)?;
		connection.pragma_update(None, "temp_store", "memory")?;
		connection.pragma_update(None, "page_size", 65536)?;
		connection.pragma_update(None, "threads", concurrency)?;

		let mut reader = TileReader {
			name: filename.to_string_lossy().to_string(),
			connection: Mutex::new(connection),
			meta_data: None,
			parameters: TileReaderParameters::new(TileFormat::PBF, Compression::None, TileBBoxPyramid::new_empty()),
		};

		reader.load_meta_data().await?;

		Ok(reader)
	}
	async fn load_meta_data(&mut self) -> Result<()> {
		trace!("load_meta_data");

		let connection = self.connection.lock().await;
		let mut stmt = connection
			.prepare("SELECT name, value FROM metadata")
			.expect("can not prepare SQL query");
		let mut entries = stmt.query([]).expect("SQL query failed");

		let mut tile_format: Option<TileFormat> = None;
		let mut compression: Option<Compression> = None;

		while let Some(entry) = entries.next()? {
			let key = entry.get::<_, String>(0)?;
			let val = entry.get::<_, String>(1)?;

			match key.as_str() {
				"format" => match val.as_str() {
					"jpg" => {
						tile_format = Some(TileFormat::JPG);
						compression = Some(Compression::None);
					}
					"pbf" => {
						tile_format = Some(TileFormat::PBF);
						compression = Some(Compression::Gzip);
					}
					"png" => {
						tile_format = Some(TileFormat::PNG);
						compression = Some(Compression::None);
					}
					"webp" => {
						tile_format = Some(TileFormat::WEBP);
						compression = Some(Compression::None);
					}
					_ => panic!("unknown file format: {val}"),
				},
				"json" => self.meta_data = Some(val),
				&_ => {}
			}
		}
		drop(entries);
		drop(stmt);
		drop(connection);

		self.parameters.set_tile_format(tile_format.unwrap());
		self.parameters.set_tile_compression(compression.unwrap());
		self.parameters.set_bbox_pyramid(block_on(self.get_bbox_pyramid()));

		if self.meta_data.is_none() {
			return Err(Error::new("'json' is not defined in table 'metadata'"));
		}

		Ok(())
	}
	async fn get_bbox_pyramid(&self) -> TileBBoxPyramid {
		trace!("get_bbox_pyramid");

		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		let connection = self.connection.lock().await;

		let query = |sql1: &str, sql2: &str| -> i32 {
			let sql = if sql2.is_empty() {
				format!("SELECT {sql1} FROM tiles")
			} else {
				format!("SELECT {sql1} FROM tiles WHERE {sql2}")
			};

			trace!("SQL: {}", sql);

			connection.query_row(&sql, [], |r| r.get(0)).unwrap()
		};

		let z0 = query("MIN(zoom_level)", "");
		let z1 = query("MAX(zoom_level)", "");

		let mut progress = ProgressBar::new("get mbtiles bbox pyramid", (z1 - z0 + 1) as u64);

		for z in z0..=z1 {
			let x0 = query("MIN(tile_column)", &format!("zoom_level = {z}"));
			let x1 = query("MAX(tile_column)", &format!("zoom_level = {z}"));
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

			let sql_prefix = format!("zoom_level = {z} AND tile_");
			let mut y0 = query("MIN(tile_row)", &format!("{sql_prefix}column = {xc}"));
			let mut y1 = query("MAX(tile_row)", &format!("{sql_prefix}column = {xc}"));

			y0 = query("MIN(tile_row)", &format!("{sql_prefix}row <= {y0}"));
			y1 = query("MAX(tile_row)", &format!("{sql_prefix}row >= {y1}"));

			let max_value = 2i32.pow(z as u32) - 1;

			bbox_pyramid.set_level_bbox(
				z as u8,
				TileBBox::new(
					z as u8,
					x0.clamp(0, max_value) as u64,
					(max_value - y1).clamp(0, max_value) as u64,
					x1.clamp(0, max_value) as u64,
					(max_value - y0).clamp(0, max_value) as u64,
				),
			);

			progress.inc(1);
		}

		progress.finish();

		bbox_pyramid
	}
}

#[async_trait]
impl TileReaderTrait for TileReader {
	async fn new(path: &str) -> Result<TileReaderBox> {
		trace!("open {}", path);

		let mut filename = current_dir()?;
		filename.push(Path::new(path));

		assert!(filename.exists(), "file {filename:?} does not exist");
		assert!(filename.is_absolute(), "path {filename:?} must be absolute");

		filename = filename.canonicalize()?;

		Ok(Box::new(Self::load_from_sqlite(&filename).await?))
	}
	fn get_container_name(&self) -> Result<&str> {
		Ok("mbtiles")
	}
	async fn get_meta(&self) -> Result<Blob> {
		Ok(Blob::from(self.meta_data.as_ref().unwrap()))
	}
	fn get_parameters(&self) -> Result<&TileReaderParameters> {
		Ok(&self.parameters)
	}
	fn get_parameters_mut(&mut self) -> Result<&mut TileReaderParameters> {
		Ok(&mut self.parameters)
	}
	async fn get_tile_data(&mut self, coord_in: &TileCoord3) -> Option<Blob> {
		trace!("read 1 tile {:?}", coord_in);

		let connection = self.connection.lock().await;
		let mut stmt = connection
			.prepare("SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?")
			.expect("SQL preparation failed");

		let mut coord: TileCoord3 = *coord_in;

		if self.get_parameters().unwrap().get_swap_xy() {
			coord.swap_xy();
		};

		if self.get_parameters().unwrap().get_flip_y() {
			coord.flip_y();
		};

		let max_index = 2u64.pow(coord.z as u32) - 1;
		let result = stmt.query_row([coord.x, max_index - coord.y, coord.z as u64], |entry| {
			entry.get::<_, Vec<u8>>(0)
		});

		if let Ok(vec) = result {
			Some(Blob::from(vec))
		} else {
			None
		}
	}
	async fn get_bbox_tile_vec(&mut self, zoom: u8, bbox: &TileBBox) -> Vec<(TileCoord2, Blob)> {
		trace!("read {} tiles for z:{}, bbox:{:?}", bbox.count_tiles(), zoom, bbox);

		let connection = self.connection.lock().await;
		let max_index = 2u64.pow(zoom as u32) - 1;

		let sql = "SELECT tile_column, tile_row, tile_data
			FROM tiles
			WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?";

		trace!("SQL: {}", sql);

		let mut stmt = connection.prepare(sql).expect("SQL preparation failed");

		let vec: Vec<(TileCoord2, Blob)> = stmt
			.query_map(
				[
					bbox.x_min,
					bbox.x_max,
					max_index - bbox.y_max,
					max_index - bbox.y_min,
					zoom.into(),
				],
				|row| {
					Ok((
						TileCoord2::new(row.get::<_, u64>(0).unwrap(), max_index - row.get::<_, u64>(1).unwrap()),
						Blob::from(row.get::<_, Vec<u8>>(2).unwrap()),
					))
				},
			)
			.unwrap()
			.map(|row| row.unwrap())
			.collect();

		trace!("result count: {}", vec.len());

		vec
	}
	fn get_name(&self) -> Result<&str> {
		Ok(&self.name)
	}
}

impl std::fmt::Debug for TileReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileReader:MBTiles")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::containers::dummy::{self, ConverterProfile};

	#[tokio::test]
	async fn reader() {
		// get test container reader
		let mut reader = TileReader::new("testdata/berlin.mbtiles").await.unwrap();

		reader.get_tile_data(&TileCoord3::new(0, 0, 0)).await;

		let mut converter = dummy::TileConverter::new_dummy(ConverterProfile::Whatever, 8);

		converter.convert_from(&mut reader).await.unwrap();
	}
}
