use crate::{
	containers::{TileReaderBox, TileReaderTrait, TileStream},
	create_error,
	shared::*,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use log::trace;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
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

		self.parameters.tile_format = tile_format.unwrap();
		self.parameters.tile_compression = compression.unwrap();
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

			bbox_pyramid.set_level_bbox(TileBBox::new(
				z as u8,
				x0.clamp(0, max_value) as u32,
				(max_value - y1).clamp(0, max_value) as u32,
				x1.clamp(0, max_value) as u32,
				(max_value - y0).clamp(0, max_value) as u32,
			));

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

		if !filename.exists() {
			return create_error!("file {filename:?} does not exist");
		};
		if !filename.is_absolute() {
			return create_error!("path {filename:?} must be absolute");
		};

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
	async fn get_tile_data_original(&mut self, coord: &TileCoord3) -> Result<Blob> {
		trace!("read 1 tile {:?}", coord);

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
	async fn get_bbox_tile_stream_original<'a>(&'a mut self, bbox: TileBBox) -> TileStream {
		if bbox.is_empty() {
			return futures_util::stream::empty().boxed();
		}

		let conn = self.pool.get().unwrap();
		let mut stmt = conn
			 .prepare("SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?")
			 .unwrap();

		let vec: Vec<(TileCoord3, Blob)> = stmt
			.query_map(
				[bbox.x_min, bbox.x_max, bbox.y_min, bbox.y_max, bbox.level as u32],
				move |row| {
					let coord = TileCoord3::new(row.get::<_, u32>(0)?, row.get::<_, u32>(1)?, row.get::<_, u8>(2)?);
					let blob = Blob::from(row.get::<_, Vec<u8>>(3)?);
					Ok((coord, blob))
				},
			)
			.unwrap()
			.filter_map(|r| match r {
				Ok(ok) => Some(ok),
				Err(_) => None,
			})
			.collect();

		futures_util::stream::iter(vec).boxed()
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
	use crate::containers::dummy::{self, ConverterProfile};

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = TileReader::new("testdata/berlin.mbtiles").await?;

		assert_eq!(format!("{:?}", reader), "TileReader:MBTiles { parameters: Ok( { bbox_pyramid: [0: [0,0,0,0] (1), 1: [1,0,1,0] (1), 2: [2,1,2,1] (1), 3: [4,2,4,2] (1), 4: [8,5,8,5] (1), 5: [17,10,17,10] (1), 6: [34,20,34,21] (2), 7: [68,41,68,42] (2), 8: [137,83,137,84] (2), 9: [274,167,275,168] (4), 10: [549,335,551,336] (6), 11: [1098,670,1102,673] (20), 12: [2196,1340,2204,1346] (63), 13: [4393,2680,4409,2693] (238), 14: [8787,5361,8818,5387] (864)], decompressor: , flip_y: false, swap_xy: false, tile_compression: Gzip, tile_format: PBF }) }");
		assert_eq!(reader.get_container_name()?, "mbtiles");
		assert!(reader.get_name()?.ends_with("testdata/berlin.mbtiles"));
		assert_eq!(reader.get_meta().await?, Blob::from(b"{\"vector_layers\":[{\"id\":\"place_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"minzoom\":3,\"maxzoom\":14},{\"id\":\"boundaries\",\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"minzoom\":0,\"maxzoom\":14},{\"id\":\"boundary_labels\",\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"minzoom\":2,\"maxzoom\":14},{\"id\":\"addresses\",\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"water_lines\",\"fields\":{\"kind\":\"String\"},\"minzoom\":4,\"maxzoom\":14},{\"id\":\"water_lines_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":4,\"maxzoom\":14},{\"id\":\"street_polygons\",\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"streets_polygons_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"streets\",\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"street_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"minzoom\":10,\"maxzoom\":14},{\"id\":\"street_labels_points\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"minzoom\":12,\"maxzoom\":14},{\"id\":\"aerialways\",\"fields\":{\"kind\":\"String\"},\"minzoom\":12,\"maxzoom\":14},{\"id\":\"public_transport\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":11,\"maxzoom\":14},{\"id\":\"buildings\",\"fields\":{\"dummy\":\"Number\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"water_polygons\",\"fields\":{\"kind\":\"String\"},\"minzoom\":4,\"maxzoom\":14},{\"id\":\"ocean\",\"fields\":{},\"minzoom\":8,\"maxzoom\":14},{\"id\":\"water_polygons_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"land\",\"fields\":{\"kind\":\"String\"},\"minzoom\":7,\"maxzoom\":14},{\"id\":\"sites\",\"fields\":{\"kind\":\"String\"},\"minzoom\":14,\"maxzoom\":14}]}".to_vec()));
		assert_eq!(format!("{:?}", reader.get_parameters()?), " { bbox_pyramid: [0: [0,0,0,0] (1), 1: [1,0,1,0] (1), 2: [2,1,2,1] (1), 3: [4,2,4,2] (1), 4: [8,5,8,5] (1), 5: [17,10,17,10] (1), 6: [34,20,34,21] (2), 7: [68,41,68,42] (2), 8: [137,83,137,84] (2), 9: [274,167,275,168] (4), 10: [549,335,551,336] (6), 11: [1098,670,1102,673] (20), 12: [2196,1340,2204,1346] (63), 13: [4393,2680,4409,2693] (238), 14: [8787,5361,8818,5387] (864)], decompressor: , flip_y: false, swap_xy: false, tile_compression: Gzip, tile_format: PBF }");
		assert_eq!(reader.get_tile_compression()?, &Compression::Gzip);
		assert_eq!(reader.get_tile_format()?, &TileFormat::PBF);

		let tile = reader.get_tile_data_original(&TileCoord3::new(8803, 5376, 14)).await?;
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

	// Test tile fetching
	#[tokio::test]
	async fn probe() -> Result<()> {
		use crate::shared::PrettyPrint;

		let mut reader = TileReader::new("testdata/berlin.mbtiles").await?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"\ncontainer:\n   deep container probing is not implemented for this container format"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await,
			"\ntiles:\n   deep tile probing is not implemented for this container format"
		);

		Ok(())
	}
}
