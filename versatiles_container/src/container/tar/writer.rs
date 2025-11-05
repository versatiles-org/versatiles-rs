//! Provides functionality for writing tile data to a tar archive.

use crate::{ProcessingConfig, TilesReaderTrait, TilesReaderTraverseExt, TilesWriterTrait};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::lock::Mutex;
use std::{
	fs::File,
	path::{Path, PathBuf},
	sync::Arc,
};
use tar::{Builder, Header};
use versatiles_core::{Traversal, io::DataWriterTrait, utils::compress};
use versatiles_derive::context;

/// A struct that provides functionality to write tile data to a tar archive.
pub struct TarTilesWriter {}

#[async_trait]
impl TilesWriterTrait for TarTilesWriter {
	/// Writes the tile data from the `TilesReader` to a tar archive at the specified path.
	///
	/// # Arguments
	/// * `reader` - The `TilesReader` instance containing the tile data.
	/// * `path` - The path to the output tar archive file.
	///
	/// # Errors
	/// Returns an error if there is an issue creating the tar archive or writing the data.
	#[context("writing tar to path '{}'", path.display())]
	async fn write_to_path(reader: &mut dyn TilesReaderTrait, path: &Path, config: ProcessingConfig) -> Result<()> {
		let file = File::create(path)?;
		let mut builder = Builder::new(file);

		let parameters = reader.parameters();
		let tile_format = &parameters.tile_format.clone();
		let tile_compression = reader.parameters().tile_compression;

		let extension_format = tile_format.as_extension();
		let extension_compression = tile_compression.as_extension();

		let meta_data = compress(reader.tilejson().into(), tile_compression)?;
		let filename = format!("tiles.json{extension_compression}");
		let mut header = Header::new_gnu();
		header.set_size(meta_data.len() as u64);
		header.set_mode(0o644);
		builder.append_data(&mut header, Path::new(&filename), meta_data.as_slice())?;

		let builder_mutex = Arc::new(Mutex::new(builder));

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, mut stream| {
					let builder_mutex = Arc::clone(&builder_mutex);
					Box::pin(async move {
						let mut builder = builder_mutex.lock().await;
						while let Some((coord, tile)) = stream.next().await {
							let filename = format!(
								"./{}/{}/{}{}{}",
								coord.level, coord.x, coord.y, extension_format, extension_compression
							);
							let path = PathBuf::from(&filename);

							let blob = tile.into_blob(tile_compression)?;

							// Build header
							let mut header = Header::new_gnu();
							header.set_size(blob.len());
							header.set_mode(0o644);

							// Write blob to file
							builder.append_data(&mut header, path, blob.as_slice())?;
						}
						Ok(())
					})
				},
				config,
			)
			.await?;

		builder_mutex.lock().await.finish()?;

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
	#[context("writing tar to DataWriter")]
	async fn write_to_writer(
		_reader: &mut dyn TilesReaderTrait,
		_writer: &mut dyn DataWriterTrait,
		_config: ProcessingConfig,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockTilesReader, MockTilesWriter, TarTilesReader};
	use assert_fs::NamedTempFile;
	use versatiles_core::*;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(4),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
		})?;

		let temp_path = NamedTempFile::new("test_output.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, ProcessingConfig::default()).await?;

		let mut reader = TarTilesReader::open_path(&temp_path)?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_meta_data() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(1),
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::JSON,
		})?;

		let temp_path = NamedTempFile::new("test_meta_output.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, ProcessingConfig::default()).await?;

		let reader = TarTilesReader::open_path(&temp_path)?;
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_empty_tiles() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_empty(),
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::JSON,
		})?;

		let temp_path = NamedTempFile::new("test_empty_tiles.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, ProcessingConfig::default()).await?;

		assert_eq!(
			TarTilesReader::open_path(&temp_path).unwrap_err().to_string(),
			"no tiles found in tar"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_invalid_path() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(2),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
		})?;

		let invalid_path = Path::new("/invalid/path/output.tar");
		let result = TarTilesWriter::write_to_path(&mut mock_reader, invalid_path, ProcessingConfig::default()).await;

		assert!(result.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn test_large_tile_set() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(7),
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::PNG,
		})?;

		let temp_path = NamedTempFile::new("test_large_tiles.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, ProcessingConfig::default()).await?;

		let reader = TarTilesReader::open_path(&temp_path)?;
		assert_eq!(reader.parameters().bbox_pyramid.count_tiles(), 21845);

		Ok(())
	}

	#[tokio::test]
	async fn test_different_compressions() -> Result<()> {
		let compressions = vec![
			TileCompression::Uncompressed,
			TileCompression::Gzip,
			TileCompression::Brotli,
		];

		for tile_compression in compressions {
			let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
				bbox_pyramid: TileBBoxPyramid::new_full(2),
				tile_compression,
				tile_format: TileFormat::MVT,
			})?;

			let temp_path = NamedTempFile::new(format!("test_compression_{tile_compression:?}.tar"))?;
			TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, ProcessingConfig::default()).await?;

			let reader = TarTilesReader::open_path(&temp_path)?;
			assert_eq!(reader.parameters().tile_compression, tile_compression);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_correct_zxy_scheme() -> Result<()> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.include_coord(&TileCoord::new(3, 1, 2)?);
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid,
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::PNG,
		})?;

		let temp_path = NamedTempFile::new("test_zxy_scheme.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, ProcessingConfig::default()).await?;

		let mut filenames = tar::Archive::new(File::open(&temp_path)?)
			.entries()?
			.map(|entry| entry.unwrap().path().unwrap().to_str().unwrap().to_string())
			.collect::<Vec<_>>();
		filenames.sort();

		assert_eq!(filenames, vec!["3/1/2.png", "tiles.json"]);

		Ok(())
	}
}
