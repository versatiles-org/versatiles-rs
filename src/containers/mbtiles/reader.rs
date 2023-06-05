use crate::{
	containers::{TileReaderBox, TileReaderTrait},
	shared::{
		Blob, Compression, Error, ProgressBar, Result, TileBBox, TileBBoxPyramid, TileCoord3, TileFormat,
		TileReaderParameters,
	},
};
use async_trait::async_trait;
use futures::Stream;
use log::trace;
use sqlx::{query_as, query_scalar, SqlitePool};
use std::{
	env::current_dir,
	path::{Path, PathBuf},
	pin::Pin,
};
use tokio_stream::StreamExt;

pub struct TileReader {
	name: String,
	pool: SqlitePool,
	meta_data: Option<String>,
	parameters: TileReaderParameters,
}
impl TileReader {
	async fn load_from_sqlite(filename: &PathBuf) -> Result<TileReader> {
		trace!("load_from_sqlite {:?}", filename);

		let name = filename.to_string_lossy().to_string();
		let pool = SqlitePool::connect(&name).await?;

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

		let pyramide = self.get_bbox_pyramid().await;
		let entries: Vec<RecordMetadata> = sqlx::query_as!(RecordMetadata, "SELECT name, value FROM metadata")
			.fetch_all(&self.pool)
			.await?;

		let mut tile_format: Option<TileFormat> = None;
		let mut compression: Option<Compression> = None;

		for entry in entries {
			let key = entry.name.unwrap();
			let val = entry.value.unwrap();

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

		self.parameters.set_tile_format(tile_format.unwrap());
		self.parameters.set_tile_compression(compression.unwrap());
		self.parameters.set_bbox_pyramid(pyramide);

		if self.meta_data.is_none() {
			return Err(Error::new("'json' is not defined in table 'metadata'"));
		}

		Ok(())
	}
	async fn simple_query(&self, sql1: &str, sql2: &str) -> i32 {
		let sql = if sql2.is_empty() {
			format!("SELECT {sql1} FROM tiles")
		} else {
			format!("SELECT {sql1} FROM tiles WHERE {sql2}")
		};

		trace!("SQL: {}", sql);

		//connection.query_row(&sql, [], |r| r.get(0)).unwrap()
		query_scalar(&sql).fetch_one(&self.pool).await.unwrap()
	}
	async fn get_bbox_pyramid(&self) -> TileBBoxPyramid {
		trace!("get_bbox_pyramid");

		let mut bbox_pyramid = TileBBoxPyramid::new_empty();

		let z0 = self.simple_query("MIN(zoom_level)", "").await;
		let z1 = self.simple_query("MAX(zoom_level)", "").await;

		let mut progress = ProgressBar::new("get mbtiles bbox pyramid", (z1 - z0 + 1) as u64);

		for z in z0..=z1 {
			let x0 = self
				.simple_query("MIN(tile_column)", &format!("zoom_level = {z}"))
				.await;
			let x1 = self
				.simple_query("MAX(tile_column)", &format!("zoom_level = {z}"))
				.await;
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
				.await;
			let mut y1 = self
				.simple_query("MAX(tile_row)", &format!("{sql_prefix}column = {xc}"))
				.await;

			y0 = self
				.simple_query("MIN(tile_row)", &format!("{sql_prefix}row <= {y0}"))
				.await;
			y1 = self
				.simple_query("MAX(tile_row)", &format!("{sql_prefix}row >= {y1}"))
				.await;

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

		let entry:RecordTile = query_as!(
			RecordTile,
			"SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?",
			x
,y,
z
			
			
		)
		.fetch_one(&self.pool)
		.await?;

		Ok(Blob::from(entry.tile_data.unwrap()))
	}
	async fn get_bbox_tile_stream<'a>(
		&'a mut self, bbox: &TileBBox,
	) -> Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + 'a + Send>> {
		let max = bbox.get_max();
		let x_min = bbox.get_x_min();
		let x_max = bbox.get_x_max();
		let y_min = max - bbox.get_y_max();
		let y_max = max - bbox.get_y_min();
		let level = bbox.get_level();

		let stream = sqlx::query_as::<_, RecordTile>(
			"SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?")
			.bind(x_min)
			.bind(x_max)
			.bind(y_min)
			.bind(y_max)
			.bind(level)
		.fetch(&self.pool);

		let stream2 = stream.map(move |r: sqlx::Result<RecordTile>| {
			let r = r.unwrap();
			let coord = TileCoord3::new(
				r.tile_column.unwrap() as u32,
				max - r.tile_row.unwrap() as u32,
				r.zoom_level.unwrap() as u8,
			);
			let blob = Blob::from(r.tile_data.unwrap());
			(coord, blob)
		});

		Box::pin(stream2)
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
	name: Option<String>,
	value: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RecordTile {
	tile_column: Option<i64>,
	tile_row: Option<i64>,
	zoom_level: Option<i64>,
	tile_data: Option<Vec<u8>>,
}

#[cfg(test)]
pub mod tests {
	use super::*;
	use crate::containers::dummy::{self, ConverterProfile};

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = TileReader::new("testdata/berlin.mbtiles").await?;

		reader.get_tile_data(&TileCoord3::new(0, 0, 0)).await?;

		let mut converter = dummy::TileConverter::new_dummy(ConverterProfile::Whatever, 8);

		converter.convert_from(&mut reader).await?;

		Ok(())
	}
}
