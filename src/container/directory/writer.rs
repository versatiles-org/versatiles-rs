#[cfg(feature = "full")]
use crate::helper::ProgressBar;
use crate::{
	container::{TilesReaderBox, TilesWriterBox, TilesWriterParameters, TilesWriterTrait},
	helper::compress,
	types::{compression_to_extension, format_to_extension, Blob},
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures::StreamExt;
use std::{
	fs,
	path::{Path, PathBuf},
};

pub struct DirectoryTilesWriter {
	dir: PathBuf,
	parameters: TilesWriterParameters,
}

impl DirectoryTilesWriter {
	fn write(&self, filename: &str, blob: Blob) -> Result<()> {
		let path = self.dir.join(filename);
		Self::ensure_directory(&path)?;
		fs::write(&path, blob.as_slice())?;
		Ok(())
	}
	fn ensure_directory(path: &Path) -> Result<()> {
		let parent = path.parent().unwrap();
		if parent.is_dir() && parent.exists() {
			return Ok(());
		}
		fs::create_dir_all(parent)?;
		Ok(())
	}
	pub fn open_path(path: &Path, parameters: TilesWriterParameters) -> Result<TilesWriterBox>
	where
		Self: Sized,
	{
		log::trace!("new {:?}", path);
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		Ok(Box::new(DirectoryTilesWriter {
			dir: path.to_path_buf(),
			parameters,
		}))
	}
}

#[async_trait]
impl TilesWriterTrait for DirectoryTilesWriter {
	fn get_parameters(&self) -> &TilesWriterParameters {
		&self.parameters
	}

	async fn write_tiles(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		log::trace!("convert_from");

		let tile_compression = &self.parameters.tile_compression;
		let tile_format = &self.parameters.tile_format;
		let bbox_pyramid = &reader.get_parameters().bbox_pyramid.clone();

		let extension_format = format_to_extension(tile_format);
		let extension_compression = compression_to_extension(tile_compression);

		let meta_data_option = reader.get_meta().await?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data, tile_compression)?;
			let filename = format!("tiles.json{extension_compression}");

			self.write(&filename, meta_data)?;
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
				self.write(&filename, blob)?;
			}
		}

		#[cfg(feature = "full")]
		bar.finish();

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		container::{MockTilesReader, TilesReaderParameters, MOCK_BYTES_PBF},
		helper::decompress_gzip,
		types::{TileBBoxPyramid, TileCompression, TileFormat},
	};
	use assert_fs;

	#[test]
	fn test_ensure_directory() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let nested_dir_path = temp_dir.path().join("a/b/c");
		assert!(!nested_dir_path.exists());

		DirectoryTilesWriter::ensure_directory(&nested_dir_path)?;

		assert!(nested_dir_path.parent().unwrap().exists());
		Ok(())
	}

	#[tokio::test]
	async fn test_convert_from() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let temp_path = temp_dir.path();

		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters::new(
			TileFormat::PBF,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full(2),
		));

		let mut writer = DirectoryTilesWriter::open_path(
			&temp_path,
			TilesWriterParameters::new(TileFormat::PBF, TileCompression::Gzip),
		)?;

		writer.write_from_reader(&mut mock_reader).await?;

		let load = |filename| {
			let path = temp_path.join(filename);
			path.try_exists().expect(&format!("filename {filename} should exist"));
			decompress_gzip(Blob::from(
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
