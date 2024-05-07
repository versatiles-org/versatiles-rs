use crate::{
	containers::{TilesReaderBox, TilesWriterBox, TilesWriterParameters, TilesWriterTrait},
	shared::{compress, compression_to_extension, format_to_extension, ProgressBar},
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::{
	fs,
	path::{Path, PathBuf},
};
use tokio::sync::Mutex;

pub struct DirectoryTilesWriter {
	dir: PathBuf,
	parameters: TilesWriterParameters,
}

impl DirectoryTilesWriter {
	fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
		let path_buf = self.dir.join(path);
		Self::ensure_directory(&path_buf.to_path_buf())?;
		fs::write(path_buf, contents)?;
		Ok(())
	}
	fn ensure_directory(path: &Path) -> Result<()> {
		let parent = path.parent().unwrap();
		if parent.is_dir() {
			return Ok(());
		}
		Self::ensure_directory(parent)?;
		fs::create_dir(parent)?;
		Ok(())
	}
}

impl DirectoryTilesWriter {
	pub fn open_file(path: &Path, parameters: TilesWriterParameters) -> Result<TilesWriterBox>
	where
		Self: Sized,
	{
		log::trace!("new {:?}", path);
		ensure!(path.is_dir(), "path {path:?} must be a directory");
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
			let filename = format!("tiles.json{}", extension_compression);

			self.write(Path::new(&filename), meta_data.as_slice())?;
		}

		let mut bar = ProgressBar::new("converting tiles", bbox_pyramid.count_tiles());
		let mutex_bar = &Mutex::new(&mut bar);

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(*bbox).await;

			while let Some(entry) = stream.next().await {
				let (coord, blob) = entry;
				mutex_bar.lock().await.inc(1);

				let filename = format!(
					"./{}/{}/{}{}{}",
					coord.get_z(),
					coord.get_y(),
					coord.get_x(),
					extension_format,
					extension_compression
				);
				let path = PathBuf::from(&filename);

				// Write blob to file
				self.write(&path, blob.as_slice())?;
			}
		}

		bar.finish();

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use crate::containers::{MockTilesReader, MockTilesReaderProfile, MOCK_BYTES_PNG};

	use super::*;
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
		let parameters = TilesWriterParameters::new(crate::shared::TileFormat::PBF, crate::shared::Compression::Gzip);
		let mut tile_converter = DirectoryTilesWriter::open_file(&temp_path, parameters)?;

		let mut mock_reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG);

		tile_converter.write_from_reader(&mut mock_reader).await?;

		assert_eq!(fs::read_to_string(temp_path.join("tiles.json"))?, "dummy meta data");
		assert_eq!(fs::read(temp_path.join("0/0/0.png"))?, MOCK_BYTES_PNG);
		assert_eq!(fs::read(temp_path.join("3/7/7.png"))?, MOCK_BYTES_PNG);

		Ok(())
	}
}
