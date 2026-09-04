//! A [`TileSink`] implementation that appends tiles to a `.tar` archive.
//!
//! Uses the same `{z}/{x}/{y}.<format>[.<compression>]` layout as [`TarTilesWriter`](super::TarTilesWriter).
//! Thread-safe via an internal async `Mutex` around the `tokio_tar::Builder`.
//!
//! Supports both local paths and `sftp://` URLs as output destinations.

use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::{
	fs::File,
	io::{AsyncWrite, AsyncWriteExt, BufWriter},
	sync::Mutex,
};
use tokio_tar::{Builder, Header};
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress};

use crate::TileSink;

/// A tile sink that writes pre-compressed blobs into a `.tar` archive.
///
/// Constructed with a fixed `TileFormat` and `TileCompression`. Every blob
/// passed to [`write_tile`](TileSink::write_tile) must already be encoded
/// and compressed accordingly.
///
/// # Thread Safety
///
/// Uses a `tokio::sync::Mutex` around the inner `tokio_tar::Builder`, so that
/// concurrent `write_tile` callers serialise on the archive without blocking a
/// worker thread — the lock is held across the append's await points.
pub struct TarTileSink<W: AsyncWrite + Send + Unpin> {
	builder: Mutex<Builder<W>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

impl TarTileSink<BufWriter<File>> {
	/// Create a new `TarTileSink` that writes to a local file at `path`.
	pub async fn new(path: &Path, tile_format: TileFormat, tile_compression: TileCompression) -> Result<Self> {
		let file = File::create(path)
			.await
			.with_context(|| format!("Failed to create output file: {}", path.display()))?;
		Ok(Self::from_writer(BufWriter::new(file), tile_format, tile_compression))
	}

	/// Open a tar tile sink from a destination string (local path or `sftp://` URL).
	pub async fn open(
		destination: &str,
		tile_format: TileFormat,
		tile_compression: TileCompression,
		runtime: &crate::TilesRuntime,
	) -> Result<Box<dyn TileSink>> {
		if destination.starts_with("sftp://") {
			#[cfg(feature = "sftp")]
			{
				let url = reqwest::Url::parse(destination)?;
				let stream = versatiles_core::io::SftpWriteStream::from_url(&url, runtime.ssh_identity()).await?;
				return Ok(Box::new(TarTileSink::from_writer(
					BufWriter::new(stream),
					tile_format,
					tile_compression,
				)));
			}
			#[cfg(not(feature = "sftp"))]
			{
				let _ = runtime;
				anyhow::bail!("SFTP support requires the 'sftp' feature");
			}
		}

		let _ = runtime;
		let path = std::env::current_dir()?.join(destination);
		Ok(Box::new(Self::new(&path, tile_format, tile_compression).await?))
	}
}

impl<W: AsyncWrite + Send + Unpin> TarTileSink<W> {
	/// Create a new `TarTileSink` from any [`AsyncWrite`] implementor.
	pub fn from_writer(writer: W, tile_format: TileFormat, tile_compression: TileCompression) -> Self {
		Self {
			// `new_non_terminated` avoids `Builder::new`'s `W: 'static` bound, which
			// comes from the task it spawns to terminate the archive on drop.
			// `finish()` below writes that same terminator.
			builder: Mutex::new(Builder::new_non_terminated(writer)),
			tile_format,
			tile_compression,
		}
	}
}

#[async_trait]
impl<W: AsyncWrite + Send + Unpin + 'static> TileSink for TarTileSink<W> {
	async fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		let filename = format!(
			"./{}/{}/{}{}{}",
			coord.level,
			coord.x,
			coord.y,
			self.tile_format.as_extension(),
			self.tile_compression.as_extension()
		);

		let mut header = Header::new_gnu();
		header.set_size(blob.len());
		header.set_mode(0o644);

		self
			.builder
			.lock()
			.await
			.append_data(&mut header, Path::new(&filename), blob.as_slice())
			.await?;
		Ok(())
	}

	async fn finish(self: Box<Self>, tilejson: &TileJSON, _runtime: &crate::TilesRuntime) -> Result<()> {
		let mut builder = self.builder.into_inner();

		// Write tiles.json metadata entry
		let meta_blob = compress(Blob::from(tilejson), &self.tile_compression)?;
		let filename = format!("tiles.json{}", self.tile_compression.as_extension());
		let mut header = Header::new_gnu();
		header.set_size(meta_blob.len());
		header.set_mode(0o644);
		builder
			.append_data(&mut header, Path::new(&filename), meta_blob.as_slice())
			.await?;

		// `into_inner` finishes the archive, then the writer is shut down
		// explicitly: `BufWriter` and the SFTP stream both buffer, and neither has
		// a destructor that can await its way to the network.
		let mut writer = builder.into_inner().await?;
		writer.shutdown().await?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TarTilesReader, TileSource, TilesRuntime};

	#[tokio::test]
	async fn write_and_read_back() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink.tar")?;

		let sink = TarTileSink::new(&temp, TileFormat::PNG, TileCompression::Uncompressed).await?;

		let coord = TileCoord::new(3, 1, 2)?;
		let blob = Blob::from(vec![0u8; 16]);
		sink.write_tile(&coord, &blob).await?;

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson, &TilesRuntime::default()).await?;

		let reader = TarTilesReader::open(&temp).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::PNG);
		assert_eq!(reader.metadata().tile_compression(), &TileCompression::Uncompressed);
		assert_eq!(reader.tile_pyramid().await?.count_tiles(), 1);

		Ok(())
	}

	#[tokio::test]
	async fn write_multiple_tiles() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink_multi.tar")?;

		let sink = TarTileSink::new(&temp, TileFormat::WEBP, TileCompression::Brotli).await?;

		for y in 0..4 {
			for x in 0..4 {
				let coord = TileCoord::new(2, x, y)?;
				#[expect(clippy::cast_possible_truncation, reason = "`x` runs 0..4")]
				let blob = Blob::from(vec![x as u8; 8]);
				sink.write_tile(&coord, &blob).await?;
			}
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson, &TilesRuntime::default()).await?;

		let reader = TarTilesReader::open(&temp).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::WEBP);
		assert_eq!(reader.metadata().tile_compression(), &TileCompression::Brotli);
		assert_eq!(reader.tile_pyramid().await?.count_tiles(), 16);

		Ok(())
	}
}
