#[cfg(feature = "full")]
use crate::helper::progress_bar::ProgressBar;
use crate::{
	container::{TilesReader, TilesWriter},
	helper::{compress, DataWriterTrait},
	types::{compression_to_extension, format_to_extension, Blob},
};
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::{
	fs,
	path::{Path, PathBuf},
};

pub struct DirectoryTilesWriter {}

impl DirectoryTilesWriter {
	fn write(path: PathBuf, blob: Blob) -> Result<()> {
		let parent = path.parent().unwrap();
		if !parent.exists() {
			fs::create_dir_all(parent)?;
		}

		fs::write(&path, blob.as_slice())?;
		Ok(())
	}
}

#[async_trait]
impl TilesWriter for DirectoryTilesWriter {
	async fn write_to_path(reader: &mut dyn TilesReader, path: &Path) -> Result<()> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		log::trace!("convert_from");

		let parameters = reader.get_parameters();
		let tile_compression = &parameters.tile_compression;
		let tile_format = &parameters.tile_format;
		let bbox_pyramid = &reader.get_parameters().bbox_pyramid.clone();

		let extension_format = format_to_extension(tile_format);
		let extension_compression = compression_to_extension(tile_compression);

		let meta_data_option = reader.get_meta()?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data, tile_compression)?;
			let filename = format!("tiles.json{extension_compression}");

			Self::write(path.join(filename), meta_data)?;
		}

		#[cfg(feature = "full")]
		let mut bar = ProgressBar::new("converting tiles", bbox_pyramid.count_tiles());

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox).await;

			while let Some(entry) = stream.next().await {
				let (coord, blob) = entry;

				#[cfg(feature = "full")]
				bar.inc(1);

				let filename = format!(
					"{}/{}/{}{}{}",
					coord.get_z(),
					coord.get_y(),
					coord.get_x(),
					extension_format,
					extension_compression
				);

				// Write blob to file
				Self::write(path.join(filename), blob)?;
			}
		}

		#[cfg(feature = "full")]
		bar.finish();

		Ok(())
	}
	async fn write_to_writer(_reader: &mut dyn TilesReader, _writer: &mut dyn DataWriterTrait) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		container::{
			mock::{MockTilesReader, MOCK_BYTES_PBF},
			TilesReaderParameters,
		},
		helper::decompress_gzip,
		types::{TileBBoxPyramid, TileCompression, TileFormat},
	};
	use assert_fs;

	#[tokio::test]
	async fn test_convert_from() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let temp_path = temp_dir.path();

		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters::new(
			TileFormat::PBF,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full(2),
		))?;

		DirectoryTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

		let load = |filename| {
			let path = temp_path.join(filename);
			path.try_exists().expect(&format!("filename {filename} should exist"));
			decompress_gzip(&Blob::from(
				fs::read(path).expect(&format!("filename {filename} should be readable")),
			))
			.expect(&format!("filename {filename} should be gzip compressed"))
		};

		assert_eq!(load("tiles.json.gz").as_str(), "dummy meta data");
		assert_eq!(load("0/0/0.pbf.gz").as_slice(), MOCK_BYTES_PBF);
		assert_eq!(load("2/3/3.pbf.gz").as_slice(), MOCK_BYTES_PBF);

		Ok(())
	}
}
