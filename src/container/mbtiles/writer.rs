use crate::{
	container::{TilesReader, TilesWriter},
	types::{progress::get_progress_bar, Blob, DataWriterTrait, TileCompression, TileCoord3, TileFormat},
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use r2d2::Pool;
use r2d2_sqlite::{rusqlite::params, SqliteConnectionManager};
use std::path::Path;

pub struct MBTilesWriter {
	pool: Pool<SqliteConnectionManager>,
}

impl MBTilesWriter {
	/// Creates a new MBTilesWriter.
	fn new(path: &Path) -> Result<Self> {
		let manager = SqliteConnectionManager::file(path);
		let pool = Pool::builder().max_size(10).build(manager)?;

		pool.get()?.execute_batch(
			"
			  CREATE TABLE IF NOT EXISTS metadata (name text, value text, UNIQUE (name));
			  CREATE TABLE IF NOT EXISTS tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
			  CREATE UNIQUE INDEX IF NOT EXISTS tile_index on tiles (zoom_level, tile_column, tile_row);
			  ",
		)?;

		Ok(MBTilesWriter { pool })
	}

	/// Adds multiple tiles to the MBTiles file within a single transaction.
	fn add_tiles(&mut self, tiles: &Vec<(TileCoord3, Blob)>) -> Result<()> {
		let mut conn = self.pool.get()?;
		let transaction = conn.transaction()?;
		for (coords, blob) in tiles {
			transaction.execute(
				"INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
				params![coords.z, coords.x, coords.y, blob.as_slice()],
			)?;
		}
		transaction.commit()?;
		Ok(())
	}

	/// Sets metadata for the MBTiles file.
	fn set_metadata(&self, name: &str, value: &str) -> Result<()> {
		self.pool.get()?.execute(
			"INSERT OR REPLACE INTO metadata (name, value) VALUES (?1, ?2)",
			params![name, value],
		)?;
		Ok(())
	}
}

#[async_trait]
impl TilesWriter for MBTilesWriter {
	async fn write_to_path(reader: &mut dyn TilesReader, path: &Path) -> Result<()> {
		let mut writer = MBTilesWriter::new(path)?;

		let parameters = reader.get_parameters().clone();

		let format = match (parameters.tile_format, parameters.tile_compression) {
			(TileFormat::JPG, TileCompression::None) => "jpg",
			(TileFormat::PBF, TileCompression::Gzip) => "pbf",
			(TileFormat::PNG, TileCompression::None) => "png",
			(TileFormat::WEBP, TileCompression::None) => "webp",
			_ => bail!(
				"combination of format ({}) and compression ({}) is not supported. MBTiles supports only uncompressed jpg/png/webp or gzipped pbf",
				parameters.tile_format,
				parameters.tile_compression
			),
		};

		writer.set_metadata("format", format)?;

		if let Some(meta_data) = reader.get_meta()? {
			writer.set_metadata("json", meta_data.as_str())?;
		}

		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();
		let mut progress = get_progress_bar("converting tiles", bbox_pyramid.count_tiles());

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox).await;

			let mut tile_buffer = Vec::new();
			while let Some((coord, blob)) = stream.next().await {
				tile_buffer.push((coord, blob));
				progress.inc(1);

				if tile_buffer.len() >= 2000 {
					writer.add_tiles(&tile_buffer)?;
					tile_buffer.clear();
				}
			}
			if !tile_buffer.is_empty() {
				writer.add_tiles(&tile_buffer)?;
			}
		}

		progress.finish();

		Ok(())
	}

	async fn write_to_writer(_reader: &mut dyn TilesReader, _writer: &mut dyn DataWriterTrait) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use assert_fs::NamedTempFile;

	use crate::{
		container::{
			mbtiles::MBTilesReader,
			mock::{MockTilesReader, MockTilesWriter},
			TilesReaderParameters,
		},
		types::{TileBBoxPyramid, TileCompression, TileFormat},
	};

	use super::*;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(5),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::PBF,
		})?;

		let filename = NamedTempFile::new("temp.mbtiles")?;
		MBTilesWriter::write_to_path(&mut mock_reader, &filename).await?;

		let mut reader = MBTilesReader::open_path(&filename)?;

		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}
}
