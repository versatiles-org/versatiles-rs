//! Write tiles and metadata into a `.tar` archive.
//!
//! The `TarTilesWriter` emits a directory-like tile pyramid into a tarball using the
//! `{z}/{x}/{y}.<format>[.<compression>]` layout and writes TileJSON as `tiles.json[.<compression>]`.
//! The transport **compression** (`.br`/`.gz` or none) follows the source reader’s
//! [`TileSourceMetadata::tile_compression`].
//!
//! ## Behavior
//! - Creates regular file entries with mode `0644`.
//! - Uses the **same** tile `format` and `compression` for all files (as reported by the reader).
//! - Writes TileJSON first, then streams all tiles from the reader (order is not significant).
//! - The output path can be relative or absolute; parent directories must exist or be creatable.
//!
//! ## Errors
//! Returns errors if the archive file cannot be created, or if encoding/compression of
//! tiles/TileJSON fails while streaming from the reader.

use crate::{TileSourceTrait, TilesReaderTraverseExt, TilesRuntime, TilesWriterTrait, Traversal};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::lock::Mutex;
use std::{
	fs::File,
	path::{Path, PathBuf},
	sync::Arc,
};
use tar::{Builder, Header};
use versatiles_core::{io::DataWriterTrait, utils::compress};
use versatiles_derive::context;

/// Writer for tiles packaged inside a tar archive.
///
/// Serializes TileJSON as `tiles.json[.<br|gz>]` and each tile as `{z}/{x}/{y}.<ext>[.<br|gz>]`,
/// using the reader’s reported `tile_format` and `tile_compression`.
///
/// Internally uses a mutex around the tar `Builder` to allow asynchronous streaming
/// of tiles while maintaining a single-writer model.
pub struct TarTilesWriter {}

#[async_trait]
impl TilesWriterTrait for TarTilesWriter {
	/// Write all tiles and TileJSON from `reader` into a tarball at `path`.
	///
	/// * Encodes TileJSON to a blob using `reader.parameters().tile_compression` and writes it as `tiles.json[.<compression>]`.
	/// * Streams all tiles from the reader and writes them to `{z}/{x}/{y}.<format>[.<compression>]`.
	/// * Creates entries with mode `0644` and writes them as regular files.
	///
	/// # Errors
	/// Returns an error if the output file cannot be created, or if any tile/metadata
	/// serialization or compression fails.
	#[context("writing tar to path '{}'", path.display())]
	async fn write_to_path(reader: &mut dyn TileSourceTrait, path: &Path, runtime: TilesRuntime) -> Result<()> {
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
				runtime.clone(),
				None,
			)
			.await?;

		builder_mutex.lock().await.finish()?;

		Ok(())
	}

	/// Not implemented: streaming a tar archive to an abstract `DataWriterTrait`.
	///
	/// # Errors
	/// Always returns `not implemented`.
	#[context("writing tar to DataWriter")]
	async fn write_to_writer(
		_reader: &mut dyn TileSourceTrait,
		_writer: &mut dyn DataWriterTrait,
		_runtime: TilesRuntime,
	) -> Result<()> {
		bail!("not implemented")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockTilesReader, MockTilesWriter, TarTilesReader, TileSourceMetadata};
	use assert_fs::NamedTempFile;
	use versatiles_core::*;

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_full(4),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
		})?;

		let temp_path = NamedTempFile::new("test_output.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, TilesRuntime::default()).await?;

		let mut reader = TarTilesReader::open_path(&temp_path)?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_meta_data() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_full(1),
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::JSON,
		})?;

		let temp_path = NamedTempFile::new("test_meta_output.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, TilesRuntime::default()).await?;

		let reader = TarTilesReader::open_path(&temp_path)?;
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_empty_tiles() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_empty(),
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::JSON,
		})?;

		let temp_path = NamedTempFile::new("test_empty_tiles.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, TilesRuntime::default()).await?;

		assert_eq!(
			TarTilesReader::open_path(&temp_path)
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"no tiles found in tar"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_invalid_path() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_full(2),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::MVT,
		})?;

		let invalid_path = Path::new("/invalid/path/output.tar");
		let result = TarTilesWriter::write_to_path(&mut mock_reader, invalid_path, TilesRuntime::default()).await;

		assert!(result.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn test_large_tile_set() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
			bbox_pyramid: TileBBoxPyramid::new_full(7),
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::PNG,
		})?;

		let temp_path = NamedTempFile::new("test_large_tiles.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, TilesRuntime::default()).await?;

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
			let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
				bbox_pyramid: TileBBoxPyramid::new_full(2),
				tile_compression,
				tile_format: TileFormat::MVT,
			})?;

			let temp_path = NamedTempFile::new(format!("test_compression_{tile_compression:?}.tar"))?;
			TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, TilesRuntime::default()).await?;

			let reader = TarTilesReader::open_path(&temp_path)?;
			assert_eq!(reader.parameters().tile_compression, tile_compression);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_correct_zxy_scheme() -> Result<()> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.include_coord(&TileCoord::new(3, 1, 2)?);
		let mut mock_reader = MockTilesReader::new_mock(TileSourceMetadata {
			bbox_pyramid,
			tile_compression: TileCompression::Uncompressed,
			tile_format: TileFormat::PNG,
		})?;

		let temp_path = NamedTempFile::new("test_zxy_scheme.tar")?;
		TarTilesWriter::write_to_path(&mut mock_reader, &temp_path, TilesRuntime::default()).await?;

		let mut filenames = tar::Archive::new(File::open(&temp_path)?)
			.entries()?
			.map(|entry| entry.unwrap().path().unwrap().to_str().unwrap().to_string())
			.collect::<Vec<_>>();
		filenames.sort();

		assert_eq!(filenames, vec!["3/1/2.png", "tiles.json"]);

		Ok(())
	}
}
