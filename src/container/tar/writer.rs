//! Provides functionality for writing tile data to a tar archive.

use crate::{
	container::{TilesReader, TilesWriter},
	types::{progress::get_progress_bar, DataWriterTrait},
	utils::compress,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::{
	fs::File,
	path::{Path, PathBuf},
};
use tar::{Builder, Header};
use tokio::sync::Mutex;

/// A struct that provides functionality to write tile data to a tar archive.
pub struct TarTilesWriter {}

#[async_trait]
impl TilesWriter for TarTilesWriter {
	/// Writes the tile data from the `TilesReader` to a tar archive at the specified path.
	///
	/// # Arguments
	/// * `reader` - The `TilesReader` instance containing the tile data.
	/// * `path` - The path to the output tar archive file.
	///
	/// # Errors
	/// Returns an error if there is an issue creating the tar archive or writing the data.
	async fn write_to_path(reader: &mut dyn TilesReader, path: &Path) -> Result<()> {
		let file = File::create(path)?;
		let mut builder = Builder::new(file);

		let parameters = reader.get_parameters();
		let tile_format = &parameters.tile_format.clone();
		let tile_compression = &parameters.tile_compression.clone();
		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		let extension_format = tile_format.extension();
		let extension_compression = tile_compression.extension();

		let meta_data_option = reader.get_meta()?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data, tile_compression)?;
			let filename = format!("tiles.json{}", extension_compression);

			let mut header = Header::new_gnu();
			header.set_size(meta_data.len() as u64);
			header.set_mode(0o644);

			builder.append_data(&mut header, Path::new(&filename), meta_data.as_slice())?;
		}

		let mut progress = get_progress_bar("converting tiles", bbox_pyramid.count_tiles());
		let mutex_builder = &Mutex::new(&mut builder);

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox.clone()).await;

			while let Some(entry) = stream.next().await {
				let (coord, blob) = entry;
				progress.inc(1);

				let filename = format!(
					"./{}/{}/{}{}{}",
					coord.get_z(),
					coord.get_y(),
					coord.get_x(),
					extension_format,
					extension_compression
				);
				let path = PathBuf::from(&filename);

				// Build header
				let mut header = Header::new_gnu();
				header.set_size(blob.len());
				header.set_mode(0o644);

				// Write blob to file
				mutex_builder
					.lock()
					.await
					.append_data(&mut header, path, blob.as_slice())?;
			}
		}

		progress.finish();
		builder.finish()?;

		Ok(())
	}

	/// Writes the tile data from the `TilesReader` to the specified `DataWriterTrait`.
	///
	/// # Arguments
	/// * `reader` - The `TilesReader` instance containing the tile data.
	/// * `writer` - The `DataWriterTrait` instance where the data will be written.
	///
	/// # Errors
	/// This function is not implemented and will return an error.
	async fn write_to_writer(
		_reader: &mut dyn TilesReader, _writer: &mut dyn DataWriterTrait,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		container::{MockTilesReader, MockTilesWriter, TarTilesReader, TilesReaderParameters},
		types::{Blob, TileBBoxPyramid, TileCompression, TileFormat},
	};

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(4),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::PBF,
		})?;

		let temp_path = Path::new("test_output.tar");
		TarTilesWriter::write_to_path(&mut mock_reader, temp_path).await?;

		let mut reader = TarTilesReader::open_path(temp_path)?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_meta_data() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(1),
			tile_compression: TileCompression::None,
			tile_format: TileFormat::JSON,
		})?;

		let temp_path = Path::new("test_meta_output.tar");
		TarTilesWriter::write_to_path(&mut mock_reader, temp_path).await?;

		let reader = TarTilesReader::open_path(temp_path)?;
		assert_eq!(reader.get_meta()?, Some(Blob::from("dummy meta data")));

		Ok(())
	}
}
