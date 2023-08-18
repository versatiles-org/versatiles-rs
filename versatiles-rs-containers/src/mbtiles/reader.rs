use crate::{TileReaderBox, TileReaderTrait};
use async_trait::async_trait;
use log::trace;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use shared::{
	create_error, Blob, Compression, ProgressBar, Result, TileBBox, TileBBoxPyramid, TileCoord3, TileFormat,
	TileReaderParameters,
};
use std::{
	env::current_dir,
	path::{Path, PathBuf},
};

pub struct TileReader {
	name: String,
	pool: Pool<SqliteConnectionManager>,
	meta_data: Option<String>,
	parameters: TileReaderParameters,
}
impl TileReader {
	async fn load_from_sqlite(filename: &PathBuf) -> Result<TileReader> {
		trace!("load_from_sqlite {:?}", filename);

		let name = filename.to_string_lossy().to_string();

		let manager = SqliteConnectionManager::file(&name);
		let pool = Pool::builder().max_size(10).build(manager)?;

		let mut reader = TileReader {
			name,
			pool,
			meta_data: None,
			parameters: TileReaderParameters::new(TileFormat::PBF, Compression::None, TileBBoxPyramid::new_empty()),
		};

		reader.load_meta_data().await?;

		Ok(reader)
	}
	async fn load_meta_data(&mut self) -> Result<()> {
		trace!("load_meta_data");

		let pyramide = self.get_bbox_pyramid().await?;
		let conn = self.pool.get()?;
		let mut stmt = conn.prepare("SELECT name, value FROM metadata")?;
		let entries = stmt.query_map([], |row| {
			Ok(RecordMetadata {
				name: row.get(0)?,
				value: row.get(1)?,
			})
		})?;

		let mut tile_format: Option<TileFormat> = None;
		let mut compression: Option<Compression> = None;

		for entry in entries {
			let entry = entry?;
			match entry.name.as_str() {
				"format" => match entry.value.as_str() {
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
					_ => panic!("unknown file format: {}", entry.value),
				},
				"json" => self.meta_data = Some(entry.value),
				&_ => {}
			}
		}

		self.parameters.set_tile_format(tile_format.unwrap());
		self.parameters.set_tile_compression(compression.unwrap());
		self.parameters.set_bbox_pyramid(pyramide);

		if self.meta_data.is_none() {
			return create_error!("'json' is not defined in table 'metadata'");
		}

		Ok(())
	}
	async fn simple_query(&self, sql1: &str, sql2: &str) -> Result<i32> {
		let sql = if sql2.is_empty() {
			format!("SELECT {sql1} FROM tiles")
		} else {
			format!("SELECT {sql1} FROM tiles WHERE {sql2}")
		};

		trace!("SQL: {}", sql);

		let conn = self.pool.get()?;
		let mut stmt = conn.prepare(&sql)?;
		Ok(stmt.query_row([], |row| row.get::<_, i32>(0))?)
	}
	async fn get_bbox_pyramid(&self) -> Result<TileBBoxPyramid> {
		trace!("get_bbox_pyramid");

		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		let z0 = self.simple_query("MIN(zoom_level)", "").await?;
		let z1 = self.simple_query("MAX(zoom_level)", "").await?;

		let mut progress = ProgressBar::new("get mbtiles bbox pyramid", (z1 - z0 + 1) as u64);

		for z in z0..=z1 {
			let x0 = self
				.simple_query("MIN(tile_column)", &format!("zoom_level = {z}"))
				.await?;
			let x1 = self
				.simple_query("MAX(tile_column)", &format!("zoom_level = {z}"))
				.await?;
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
			let mut y0 = self
				.simple_query("MIN(tile_row)", &format!("{sql_prefix}column = {xc}"))
				.await?;
			let mut y1 = self
				.simple_query("MAX(tile_row)", &format!("{sql_prefix}column = {xc}"))
				.await?;

			y0 = self
				.simple_query("MIN(tile_row)", &format!("{sql_prefix}row <= {y0}"))
				.await?;
			y1 = self
				.simple_query("MAX(tile_row)", &format!("{sql_prefix}row >= {y1}"))
				.await?;

			let max_value = 2i32.pow(z as u32) - 1;

			bbox_pyramid.set_level_bbox(
				z as u8,
				TileBBox::new(
					z as u8,
					x0.clamp(0, max_value) as u32,
					(max_value - y1).clamp(0, max_value) as u32,
					x1.clamp(0, max_value) as u32,
					(max_value - y0).clamp(0, max_value) as u32,
				),
			);

			progress.inc(1);
		}

		progress.finish();

		Ok(bbox_pyramid)
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

		let db = Self::load_from_sqlite(&filename).await?;
		Ok(Box::new(db))
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
	async fn get_tile_data(&mut self, coord_in: &TileCoord3) -> Result<Blob> {
		trace!("read 1 tile {:?}", coord_in);

		let mut coord: TileCoord3 = *coord_in;

		if self.get_parameters()?.get_swap_xy() {
			coord.swap_xy();
		};

		if self.get_parameters()?.get_flip_y() {
			coord.flip_y();
		};

		let max_index = 2u32.pow(coord.get_z() as u32) - 1;
		let x = coord.get_x();
		let y = max_index - coord.get_y();
		let z = coord.get_z() as u32;

		let conn = self.pool.get()?;
		let mut stmt =
			conn.prepare("SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?")?;

		let blob = stmt.query_row([x, y, z], |row| row.get::<_, Vec<u8>>(0))?;

		Ok(Blob::from(blob))
	}
	async fn get_bbox_tile_vec(&mut self, bbox: &TileBBox) -> Result<Vec<(TileCoord3, Blob)>> {
		let max = bbox.get_max();
		let x_min = bbox.get_x_min();
		let x_max = bbox.get_x_max();
		let y_min = max - bbox.get_y_max();
		let y_max = max - bbox.get_y_min();
		let level = bbox.get_level();

		let conn = self.pool.get()?;
		let mut stmt = conn
			 .prepare("SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?")
			 ?;

		let mut rows = stmt.query([x_min, x_max, y_min, y_max, level as u32])?;

		let mut vec: Vec<(TileCoord3, Blob)> = Vec::new();

		while let Some(row) = rows.next()? {
			vec.push((
				TileCoord3::new(
					row.get::<_, u32>(0).unwrap(),
					max - row.get::<_, u32>(1).unwrap(),
					row.get::<_, u8>(2).unwrap(),
				),
				Blob::from(row.get::<_, Vec<u8>>(3)?),
			))
		}

		Ok(vec)
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

struct RecordMetadata {
	name: String,
	value: String,
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::dummy::{self, ConverterProfile};

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = TileReader::new("../testdata/berlin.mbtiles").await?;

		let tile = reader.get_tile_data(&TileCoord3::new(8803, 5376, 14)).await?;
		assert_eq!(tile.len(), 172969);
		assert_eq!(tile.get_range(0..10), &[31, 139, 8, 0, 0, 0, 0, 0, 0, 3]);
		assert_eq!(
			tile.get_range(172959..172969),
			&[255, 15, 172, 89, 205, 237, 7, 134, 5, 0]
		);

		let mut converter = dummy::TileConverter::new_dummy(ConverterProfile::Whatever, 8);

		converter.convert_from(&mut reader).await?;

		Ok(())
	}
}
