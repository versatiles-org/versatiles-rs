use crate::{
	containers::{TileConverterBox, TileConverterTrait, TileReaderBox},
	shared::{compress, compression_to_extension, format_to_extension, ProgressBar, TileConverterConfig},
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::{
	env, fs,
	path::{Path, PathBuf},
};
use tokio::sync::Mutex;

pub struct TileConverter {
	dir: PathBuf,
	config: TileConverterConfig,
}

impl TileConverter {
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

#[async_trait]
impl TileConverterTrait for TileConverter {
	async fn new(filename: &str, config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized,
	{
		log::trace!("new {:?}", filename);

		let dir = env::current_dir()?.join(filename);
		ensure!(dir.is_dir(), "path {dir:?} must be a directory");
		ensure!(dir.is_absolute(), "path {dir:?} must be absolute");

		Ok(Box::new(TileConverter { dir, config }))
	}
	async fn convert_from(&mut self, reader: &mut TileReaderBox) -> Result<()> {
		log::trace!("convert_from");

		self.config.finalize_with_parameters(reader.get_parameters());

		let tile_converter = self.config.get_tile_recompressor();

		let extension_format = format_to_extension(self.config.get_tile_format());
		let extension_compression = compression_to_extension(self.config.get_tile_compression());

		let bbox_pyramid = self.config.get_bbox_pyramid();

		let meta_data_option = reader.get_meta().await?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data, self.config.get_tile_compression())?;
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

				if let Ok(blob) = tile_converter.process_blob(blob) {
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
		}

		bar.finish();

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use assert_fs;

	use super::*;
	use crate::containers::mock;
	use std::fs::File;
	use std::io::Read;

	#[test]
	fn test_write() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let tile_converter = TileConverter {
			dir: temp_dir.path().to_path_buf(),
			config: TileConverterConfig::new_full(),
		};

		let file_path = Path::new("test_write.txt");
		let contents = b"Hello, world!";
		tile_converter.write(file_path, contents)?;

		let mut file = File::open(temp_dir.path().join(file_path))?;
		let mut file_contents = Vec::new();
		file.read_to_end(&mut file_contents)?;

		assert_eq!(contents.as_ref(), file_contents.as_slice());
		Ok(())
	}

	#[test]
	fn test_ensure_directory() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let nested_dir_path = temp_dir.path().join("a/b/c");
		assert!(!nested_dir_path.exists());

		TileConverter::ensure_directory(&nested_dir_path)?;

		assert!(nested_dir_path.parent().unwrap().exists());
		Ok(())
	}

	#[tokio::test]
	async fn test_convert_from() -> Result<()> {
		let temp_dir = assert_fs::TempDir::new()?;
		let temp_path = temp_dir.path();
		let filename = temp_path.to_str().unwrap();
		let tile_config = TileConverterConfig::new_full();
		let mut tile_converter = TileConverter::new(filename, tile_config).await?;

		let mut mock_reader = mock::TileReader::new_mock(mock::ReaderProfile::PNG, 3);

		tile_converter.convert_from(&mut mock_reader).await?;

		assert_eq!(fs::read_to_string(temp_path.join("tiles.json"))?, "dummy meta data");
		assert_eq!(fs::read(temp_path.join("0/0/0.png"))?, mock::BYTES_PNG);
		assert_eq!(fs::read(temp_path.join("3/7/7.png"))?, mock::BYTES_PNG);

		Ok(())
	}
}
