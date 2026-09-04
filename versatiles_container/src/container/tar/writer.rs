//! Write tiles and metadata into a `.tar` archive.
//!
//! The `TarTilesWriter` emits a directory-like tile pyramid into a tarball using the
//! `{z}/{x}/{y}.<format>[.<compression>]` layout and writes `TileJSON` as `tiles.json[.<compression>]`.
//! The transport **compression** (`.br`/`.gz` or none) follows the source reader’s
//! [`TileSourceMetadata::tile_compression`](crate::TileSourceMetadata::tile_compression).
//!
//! ## Behavior
//! - Creates regular file entries with mode `0644`.
//! - Uses the **same** tile `format` and `compression` for all files (as reported by the reader).
//! - Writes `TileJSON` first, then streams all tiles from the reader (order is not significant).
//! - The output path can be relative or absolute; parent directories must exist or be creatable.
//!
//! ## Errors
//! Returns errors if the archive file cannot be created, or if encoding/compression of
//! tiles/TileJSON fails while streaming from the reader.

use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

use anyhow::Result;
use async_trait::async_trait;
use futures::lock::Mutex;
use tokio::io::{AsyncReadExt, AsyncWrite};
use tokio_tar::{Builder, Header};
use versatiles_core::{Blob, compression::compress, io::DataWriterTrait};
use versatiles_derive::context;

use crate::{TileSource, TileSourceTraverseExt, TilesRuntime, TilesWriter, Traversal};

/// Writer for tiles packaged inside a tar archive.
///
/// Serializes `TileJSON` as `tiles.json[.<br|gz>]` and each tile as `{z}/{x}/{y}.<ext>[.<br|gz>]`,
/// using the reader’s reported `tile_format` and `tile_compression`.
///
/// Internally uses a mutex around the tar `Builder` to allow asynchronous streaming
/// of tiles while maintaining a single-writer model.
/// Buffer size for the duplex pipe used by [`TilesWriter::write_to_writer`], and
/// the size of each chunk appended to the writer.
const PIPE_CAPACITY: usize = 256 * 1024;

/// Writes a source into an uncompressed `.tar` archive of tile files.
pub struct TarTilesWriter {}

impl TarTilesWriter {
	async fn write_tar<W: AsyncWrite + Unpin + Send>(
		reader: &dyn TileSource,
		sink: W,
		runtime: TilesRuntime,
	) -> Result<()> {
		// `new_non_terminated` rather than `new`: `Builder::new` spawns a task that
		// owns the sink so it can write the terminator on drop, which would force a
		// `W: 'static` bound on this function and cost a task per archive. The
		// explicit `finish()` below writes that same 1024-byte terminator, so the
		// archive is identical either way.
		let mut builder = Builder::new_non_terminated(sink);

		let parameters = reader.metadata();
		let tile_format = *parameters.tile_format();
		let tile_compression = *reader.metadata().tile_compression();

		let extension_format = tile_format.as_extension();
		let extension_compression = tile_compression.as_extension();

		let meta_data = compress(reader.tilejson().into(), &tile_compression)?;
		let filename = format!("tiles.json{extension_compression}");
		let mut header = Header::new_gnu();
		header.set_size(meta_data.len() as u64);
		header.set_mode(0o644);
		builder
			.append_data(&mut header, Path::new(&filename), meta_data.as_slice())
			.await?;

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

							let blob = tile.into_blob(&tile_compression)?;

							let mut header = Header::new_gnu();
							header.set_size(blob.len());
							header.set_mode(0o644);

							builder.append_data(&mut header, path, blob.as_slice()).await?;
						}
						Ok(())
					})
				},
				runtime.clone(),
			)
			.await?;

		builder_mutex.lock().await.finish().await?;

		Ok(())
	}
}

#[async_trait]
impl TilesWriter for TarTilesWriter {
	#[context("writing tar to path '{}'", path.display())]
	async fn write_to_path(reader: &dyn TileSource, path: &Path, runtime: TilesRuntime) -> Result<()> {
		let file = tokio::fs::File::create(path).await?;
		Self::write_tar(reader, file, runtime).await
	}

	#[context("writing tar to DataWriter")]
	async fn write_to_writer(
		reader: &dyn TileSource,
		writer: &mut dyn DataWriterTrait,
		runtime: TilesRuntime,
	) -> Result<()> {
		// `DataWriterTrait` is async, so it cannot be presented as an `AsyncWrite`
		// without a hand-rolled future state machine in `poll_write`. A duplex pipe
		// avoids that: the builder writes into one end while this task drains the
		// other into the writer. `join!` rather than `spawn` because both halves
		// borrow — the reader and the writer are references.
		let (pipe, mut drain) = tokio::io::duplex(PIPE_CAPACITY);

		let build = Self::write_tar(reader, pipe, runtime);
		let forward = async {
			let mut buffer = vec![0u8; PIPE_CAPACITY];
			loop {
				let read = drain.read(&mut buffer).await?;
				if read == 0 {
					break;
				}
				writer.append(&Blob::from(&buffer[..read])).await?;
			}
			anyhow::Ok(())
		};

		let (built, forwarded) = futures::join!(build, forward);
		built?;
		forwarded?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use assert_fs::NamedTempFile;
	use futures::StreamExt;
	use versatiles_core::*;

	use super::*;
	use crate::{MockReader, MockWriter, TarTilesReader, TileSourceMetadata};

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mock_reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(4),
			TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, Traversal::ANY, None),
		)?;

		let temp_path = NamedTempFile::new("test_output.tar")?;
		TarTilesWriter::write_to_path(&mock_reader, &temp_path, TilesRuntime::default()).await?;

		let reader = TarTilesReader::open(&temp_path).await?;
		MockWriter::write(&reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_meta_data() -> Result<()> {
		let mock_reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(1),
			TileSourceMetadata::new(TileFormat::JSON, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;

		let temp_path = NamedTempFile::new("test_meta_output.tar")?;
		TarTilesWriter::write_to_path(&mock_reader, &temp_path, TilesRuntime::default()).await?;

		let reader = TarTilesReader::open(&temp_path).await?;
		assert_eq!(
			reader.tilejson().stringify(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_empty_tiles() -> Result<()> {
		let mock_reader = MockReader::new_mock(
			TilePyramid::new_empty(),
			TileSourceMetadata::new(TileFormat::JSON, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;

		let temp_path = NamedTempFile::new("test_empty_tiles.tar")?;
		TarTilesWriter::write_to_path(&mock_reader, &temp_path, TilesRuntime::default()).await?;

		assert_eq!(
			TarTilesReader::open(&temp_path)
				.await
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
		let mock_reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(2),
			TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, Traversal::ANY, None),
		)?;

		let invalid_path = Path::new("/invalid/path/output.tar");
		let result = TarTilesWriter::write_to_path(&mock_reader, invalid_path, TilesRuntime::default()).await;

		assert!(result.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn test_large_tile_set() -> Result<()> {
		let mock_reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(7),
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;

		let temp_path = NamedTempFile::new("test_large_tiles.tar")?;
		TarTilesWriter::write_to_path(&mock_reader, &temp_path, TilesRuntime::default()).await?;

		let reader = TarTilesReader::open(&temp_path).await?;
		assert_eq!(reader.tile_pyramid().await?.count_tiles(), 21845);

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
			let mock_reader = MockReader::new_mock(
				TilePyramid::new_full_up_to(2),
				TileSourceMetadata::new(TileFormat::MVT, tile_compression, Traversal::ANY, None),
			)?;

			let temp_path = NamedTempFile::new(format!("test_compression_{tile_compression:?}.tar"))?;
			TarTilesWriter::write_to_path(&mock_reader, &temp_path, TilesRuntime::default()).await?;

			let reader = TarTilesReader::open(&temp_path).await?;
			assert_eq!(reader.metadata().tile_compression(), &tile_compression);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_correct_zxy_scheme() -> Result<()> {
		let mut tile_pyramid = TilePyramid::new_empty();
		tile_pyramid.insert_coord(&TileCoord::new(3, 1, 2)?);
		let mock_reader = MockReader::new_mock(
			tile_pyramid,
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;

		let temp_path = NamedTempFile::new("test_zxy_scheme.tar")?;
		TarTilesWriter::write_to_path(&mock_reader, &temp_path, TilesRuntime::default()).await?;

		let mut archive = tokio_tar::Archive::new(tokio::fs::File::open(&temp_path).await?);
		let mut filenames = archive
			.entries()?
			.map(|entry| entry.unwrap().path().unwrap().to_str().unwrap().to_string())
			.collect::<Vec<_>>()
			.await;
		filenames.sort();

		assert_eq!(filenames, vec!["3/1/2.png", "tiles.json"]);

		Ok(())
	}
}
