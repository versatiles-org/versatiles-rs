//! Provides functionality for writing tile data to a tar archive.

use crate::TilesWriterTrait;
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::{
	fs::File,
	path::{Path, PathBuf},
};
use tar::{Builder, Header};
use versatiles_core::{io::DataWriterTrait, progress::get_progress_bar, types::TilesReaderTrait, utils::compress};

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
	async fn write_to_path(reader: &mut dyn TilesReaderTrait, path: &Path) -> Result<()> {
		let file = File::create(path)?;
		let mut builder = Builder::new(file);

		let parameters = reader.get_parameters();
		let tile_format = &parameters.tile_format.clone();
		let tile_compression = &parameters.tile_compression.clone();
		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		let extension_format = tile_format.as_extension();
		let extension_compression = tile_compression.extension();

		let meta_data = compress(reader.get_tilejson().into(), tile_compression)?;
		let filename = format!("tiles.json{extension_compression}");
		let mut header = Header::new_gnu();
		header.set_size(meta_data.len() as u64);
		header.set_mode(0o644);
		builder.append_data(&mut header, Path::new(&filename), meta_data.as_slice())?;

		let mut progress = get_progress_bar("converting tiles", bbox_pyramid.count_tiles());

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox.clone()).await;

			while let Some((coord, blob)) = stream.next().await {
				progress.inc(1);

				let filename = format!(
					"./{}/{}/{}{}{}",
					coord.z, coord.x, coord.y, extension_format, extension_compression
				);
				let path = PathBuf::from(&filename);

				// Build header
				let mut header = Header::new_gnu();
				header.set_size(blob.len());
				header.set_mode(0o644);

				// Write blob to file
				builder.append_data(&mut header, path, blob.as_slice())?;
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
	async fn write_to_writer(_reader: &mut dyn TilesReaderTrait, _writer: &mut dyn DataWriterTrait) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockTilesReader, MockTilesWriter, TarTilesReader};
	use assert_fs::NamedTempFile;
	use versatiles_core::types::*;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(4),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
		})?;

		let temp_path = NamedTempFile::new("test_output.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

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
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

		let reader = TarTilesReader::open_path(&temp_path)?;
		assert_eq!(
			reader.get_tilejson().as_string(),
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
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

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
		let result = TarTilesWriter::write_to_path(&mut mock_reader, invalid_path).await;

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
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

		let reader = TarTilesReader::open_path(&temp_path)?;
		assert_eq!(reader.get_parameters().bbox_pyramid.count_tiles(), 21845);

		Ok(())
	}

	#[tokio::test]
	async fn test_different_compressions() -> Result<()> {
		let compressions = vec![
			TileCompression::Uncompressed,
			TileCompression::Gzip,
			TileCompression::Brotli,
		];

		for compression in compressions {
			let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
				bbox_pyramid: TileBBoxPyramid::new_full(2),
				tile_compression: compression,
				tile_format: TileFormat::MVT,
			})?;

			let temp_path = NamedTempFile::new(format!("test_compression_{compression:?}.tar"))?;
			TarTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

			let reader = TarTilesReader::open_path(&temp_path)?;
			assert_eq!(reader.get_parameters().tile_compression, compression);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_correct_zxy_scheme() -> Result<()> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.include_coord(&TileCoord3::new(1, 2, 3)?);
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid,
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::PNG,
		})?;

		let temp_path = NamedTempFile::new("test_zxy_scheme.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path).await?;

		let mut filenames = tar::Archive::new(File::open(&temp_path)?)
			.entries()?
			.map(|entry| entry.unwrap().path().unwrap().to_str().unwrap().to_string())
			.collect::<Vec<_>>();
		filenames.sort();

		assert_eq!(filenames, vec!["3/1/2.png", "tiles.json"]);

		Ok(())
	}
}
