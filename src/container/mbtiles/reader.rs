use crate::{
	container::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait, TilesStream},
	helper::{ProgressBar, TransformCoord},
	types::{Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat},
};
use anyhow::{anyhow, ensure, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use log::trace;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

pub struct MBTilesReader {
	name: String,
	pool: Pool<SqliteConnectionManager>,
	meta_data: Option<String>,
	parameters: TilesReaderParameters,
}
impl MBTilesReader {
	pub async fn open(path: &Path) -> Result<TilesReaderBox> {
		trace!("open {path:?}");

		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		let mut db = Self::load_from_sqlite(path).await?;
		path.to_str().unwrap().clone_into(&mut db.name);

		Ok(Box::new(db))
	}
	async fn load_from_sqlite(path: &Path) -> Result<MBTilesReader> {
		trace!("load_from_sqlite {:?}", path);

		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;
		let parameters = TilesReaderParameters::new(TileFormat::PBF, TileCompression::None, TileBBoxPyramid::new_empty());

		let mut reader = MBTilesReader {
			name: String::from(path.to_str().unwrap()),
			pool,
			meta_data: None,
			parameters,
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

		let mut tile_format: Result<TileFormat> = Err(anyhow!("mbtiles file {} does not specify tile format", self.name));
		let mut compression: Result<TileCompression> =
			Err(anyhow!("mbtiles file {} does not specify compression", self.name));

		for entry in entries {
			let entry = entry?;
			match entry.name.as_str() {
				"format" => match entry.value.as_str() {
					"jpg" => {
						tile_format = Ok(TileFormat::JPG);
						compression = Ok(TileCompression::None);
					}
					"pbf" => {
						tile_format = Ok(TileFormat::PBF);
						compression = Ok(TileCompression::Gzip);
					}
					"png" => {
						tile_format = Ok(TileFormat::PNG);
						compression = Ok(TileCompression::None);
					}
					"webp" => {
						tile_format = Ok(TileFormat::WEBP);
						compression = Ok(TileCompression::None);
					}
					_ => panic!("unknown file format: {}", entry.value),
				},
				"json" => self.meta_data = Some(entry.value),
				&_ => {}
			}
		}

		self.parameters.tile_format = tile_format?;
		self.parameters.tile_compression = compression?;
		self.parameters.bbox_pyramid = pyramide;

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
				y0.clamp(0, max_value) as u32,
				x1.clamp(0, max_value) as u32,
				y1.clamp(0, max_value) as u32,
			)?);

			progress.inc(1);
		}

		progress.finish();

		bbox_pyramid.flip_y();

		Ok(bbox_pyramid)
	}
}

#[async_trait]
impl TilesReaderTrait for MBTilesReader {
	fn get_container_name(&self) -> &str {
		"mbtiles"
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(self.meta_data.as_ref().map(Blob::from))
	}
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		trace!("read tile from coord {coord:?}");

		trace!("corrected coord {coord:?}");

		let max_index = 2u32.pow(coord.get_z() as u32) - 1;
		let x = coord.get_x();
		let y = max_index - coord.get_y();
		let z = coord.get_z() as u32;

		let conn = self.pool.get()?;
		let mut stmt =
			conn.prepare("SELECT tile_data FROM tiles WHERE tile_column = ? AND tile_row = ? AND zoom_level = ?")?;

		let blob = stmt.query_row([x, y, z], |row| row.get::<_, Vec<u8>>(0))?;

		Ok(Some(Blob::from(blob)))
	}
	async fn get_bbox_tile_stream<'a>(&'a mut self, bbox: &TileBBox) -> TilesStream {
		trace!("read tile stream from bbox {bbox:?}");

		if bbox.is_empty() {
			return futures_util::stream::empty().boxed();
		}

		let max_index = bbox.max;

		trace!("corrected bbox {bbox:?}");

		let conn = self.pool.get().unwrap();
		let mut stmt = conn
			 .prepare("SELECT tile_column, tile_row, zoom_level, tile_data FROM tiles WHERE tile_column >= ? AND tile_column <= ? AND tile_row >= ? AND tile_row <= ? AND zoom_level = ?")
			 .unwrap();

		let vec: Vec<(TileCoord3, Blob)> = stmt
			.query_map(
				[
					bbox.x_min,
					bbox.x_max,
					max_index - bbox.y_max,
					max_index - bbox.y_min,
					bbox.level as u32,
				],
				move |row| {
					let coord = TileCoord3::new(
						row.get::<_, u32>(0)?,
						max_index - row.get::<_, u32>(1)?,
						row.get::<_, u8>(2)?,
					)
					.unwrap();
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

		trace!("got {} tiles", vec.len());

		futures_util::stream::iter(vec).boxed()
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl std::fmt::Debug for MBTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MBTilesReader")
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
	use crate::container::{MockTilesWriter, MockTilesWriterProfile};
	use lazy_static::lazy_static;
	use std::{env, path::PathBuf};

	lazy_static! {
		static ref PATH: PathBuf = env::current_dir().unwrap().join("./testdata/berlin.mbtiles");
	}

	#[tokio::test]
	async fn reader() -> Result<()> {
		// get test container reader
		let mut reader = MBTilesReader::open(&PATH).await?;

		assert_eq!(format!("{:?}", reader), "MBTilesReader { parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [1,0,1,0] (1), 2: [2,1,2,1] (1), 3: [4,2,4,2] (1), 4: [8,5,8,5] (1), 5: [17,10,17,10] (1), 6: [34,20,34,21] (2), 7: [68,41,68,42] (2), 8: [137,83,137,84] (2), 9: [274,167,275,168] (4), 10: [549,335,551,336] (6), 11: [1098,670,1102,673] (20), 12: [2196,1340,2204,1346] (63), 13: [4393,2680,4409,2693] (238), 14: [8787,5361,8818,5387] (864)], tile_compression: Gzip, tile_format: PBF } }");
		assert_eq!(reader.get_container_name(), "mbtiles");
		assert!(reader.get_name().ends_with("testdata/berlin.mbtiles"));
		assert_eq!(reader.get_meta().await?, Some(Blob::from(b"{\"vector_layers\":[{\"id\":\"place_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"population\":\"Number\"},\"minzoom\":3,\"maxzoom\":14},{\"id\":\"boundaries\",\"fields\":{\"admin_level\":\"Number\",\"maritime\":\"Boolean\"},\"minzoom\":0,\"maxzoom\":14},{\"id\":\"boundary_labels\",\"fields\":{\"admin_level\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"way_area\":\"Number\"},\"minzoom\":2,\"maxzoom\":14},{\"id\":\"addresses\",\"fields\":{\"name\":\"String\",\"number\":\"String\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"water_lines\",\"fields\":{\"kind\":\"String\"},\"minzoom\":4,\"maxzoom\":14},{\"id\":\"water_lines_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":4,\"maxzoom\":14},{\"id\":\"street_polygons\",\"fields\":{\"bridge\":\"Boolean\",\"kind\":\"String\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tunnel\":\"Boolean\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"streets_polygons_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"streets\",\"fields\":{\"bicycle\":\"String\",\"bridge\":\"Boolean\",\"horse\":\"String\",\"kind\":\"String\",\"link\":\"Boolean\",\"rail\":\"Boolean\",\"service\":\"String\",\"surface\":\"String\",\"tracktype\":\"String\",\"tunnel\":\"Boolean\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"street_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\",\"ref_cols\":\"Number\",\"ref_rows\":\"Number\",\"tunnel\":\"Boolean\"},\"minzoom\":10,\"maxzoom\":14},{\"id\":\"street_labels_points\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\",\"ref\":\"String\"},\"minzoom\":12,\"maxzoom\":14},{\"id\":\"aerialways\",\"fields\":{\"kind\":\"String\"},\"minzoom\":12,\"maxzoom\":14},{\"id\":\"public_transport\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":11,\"maxzoom\":14},{\"id\":\"buildings\",\"fields\":{\"dummy\":\"Number\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"water_polygons\",\"fields\":{\"kind\":\"String\"},\"minzoom\":4,\"maxzoom\":14},{\"id\":\"ocean\",\"fields\":{},\"minzoom\":8,\"maxzoom\":14},{\"id\":\"water_polygons_labels\",\"fields\":{\"kind\":\"String\",\"name\":\"String\",\"name_de\":\"String\",\"name_en\":\"String\"},\"minzoom\":14,\"maxzoom\":14},{\"id\":\"land\",\"fields\":{\"kind\":\"String\"},\"minzoom\":7,\"maxzoom\":14},{\"id\":\"sites\",\"fields\":{\"kind\":\"String\"},\"minzoom\":14,\"maxzoom\":14}]}".to_vec())));
		assert_eq!(format!("{:?}", reader.get_parameters()), "TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [1,0,1,0] (1), 2: [2,1,2,1] (1), 3: [4,2,4,2] (1), 4: [8,5,8,5] (1), 5: [17,10,17,10] (1), 6: [34,20,34,21] (2), 7: [68,41,68,42] (2), 8: [137,83,137,84] (2), 9: [274,167,275,168] (4), 10: [549,335,551,336] (6), 11: [1098,670,1102,673] (20), 12: [2196,1340,2204,1346] (63), 13: [4393,2680,4409,2693] (238), 14: [8787,5361,8818,5387] (864)], tile_compression: Gzip, tile_format: PBF }");
		assert_eq!(reader.get_parameters().tile_compression, TileCompression::Gzip);
		assert_eq!(reader.get_parameters().tile_format, TileFormat::PBF);

		let tile = reader.get_tile_data(&TileCoord3::new(8803, 5376, 14)?).await?.unwrap();
		assert_eq!(tile.len(), 172969);
		assert_eq!(tile.get_range(0..10), &[31, 139, 8, 0, 0, 0, 0, 0, 0, 3]);
		assert_eq!(
			tile.get_range(172959..172969),
			&[255, 15, 172, 89, 205, 237, 7, 134, 5, 0]
		);

		let mut converter = MockTilesWriter::new_mock_profile(MockTilesWriterProfile::PBF);

		converter.write_tiles(&mut reader).await?;

		Ok(())
	}

	// Test tile fetching
	#[tokio::test]
	async fn probe() -> Result<()> {
		use crate::helper::PrettyPrint;

		let mut reader = MBTilesReader::open(&PATH).await?;

		let mut printer = PrettyPrint::new();
		reader.probe_container(&printer.get_category("container").await).await?;
		assert_eq!(
			printer.as_string().await,
			"container:\n   deep container probing is not implemented for this container format\n"
		);

		let mut printer = PrettyPrint::new();
		reader.probe_tiles(&printer.get_category("tiles").await).await?;
		assert_eq!(
			printer.as_string().await,
			"tiles:\n   deep tiles probing is not implemented for this container format\n"
		);

		Ok(())
	}
}
