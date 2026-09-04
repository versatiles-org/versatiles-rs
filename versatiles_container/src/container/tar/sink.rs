//! A [`TileSink`] implementation that appends tiles to a `.tar` archive.
//!
//! Uses the same `{z}/{x}/{y}.<format>[.<compression>]` layout as [`TarTilesWriter`](super::TarTilesWriter).
//! Thread-safe via an internal `Mutex` around the `tokio_tar::Builder`.
//!
//! Supports both local paths and `sftp://` URLs as output destinations.

use std::{
	fs::File,
	io::{BufWriter, Write},
	path::Path,
	pin::Pin,
	sync::Mutex,
	task::{Context as TaskContext, Poll},
};

use anyhow::{Context, Result};
use tokio::io::AsyncWrite;
use tokio_tar::{Builder, Header};
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress};

use crate::TileSink;

/// Presents a synchronous `std::io::Write` as the [`AsyncWrite`] that
/// `tokio_tar::Builder` requires.
///
/// [`TileSink`] is synchronous while the tar builder is not, and this is the
/// join between them. It is safe in a way a general bridge would not be: the
/// inner writer is an ordinary blocking `Write`, so `poll_write` always returns
/// `Ready` and the `block_on` in the sink below never parks — it needs no
/// reactor, no runtime, and cannot deadlock even on a current-thread runtime.
///
/// Issue #271 converts `TileSink` to async, which removes this adapter.
struct SyncWriteAdapter<W>(W);

impl<W: Write + Unpin> AsyncWrite for SyncWriteAdapter<W> {
	fn poll_write(mut self: Pin<&mut Self>, _cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
		Poll::Ready(self.0.write(buf))
	}

	fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
		Poll::Ready(self.0.flush())
	}

	fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
		Poll::Ready(Ok(()))
	}
}

/// A tile sink that writes pre-compressed blobs into a `.tar` archive.
///
/// Constructed with a fixed `TileFormat` and `TileCompression`. Every blob
/// passed to [`write_tile`](TileSink::write_tile) must already be encoded
/// and compressed accordingly.
///
/// # Thread Safety
///
/// Uses a `std::sync::Mutex` around the inner `tokio_tar::Builder` so that
/// multiple threads can call `write_tile` concurrently (e.g. from
/// `spawn_blocking` tasks).
pub struct TarTileSink<W: Write + Send + Unpin> {
	builder: Mutex<Builder<SyncWriteAdapter<W>>>,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

impl TarTileSink<BufWriter<File>> {
	/// Create a new `TarTileSink` that writes to a local file at `path`.
	pub fn new(path: &Path, tile_format: TileFormat, tile_compression: TileCompression) -> Result<Self> {
		let file = File::create(path).with_context(|| format!("Failed to create output file: {}", path.display()))?;
		Ok(Self::from_writer(BufWriter::new(file), tile_format, tile_compression))
	}

	/// Open a tar tile sink from a destination string (local path or `sftp://` URL).
	pub fn open(
		destination: &str,
		tile_format: TileFormat,
		tile_compression: TileCompression,
		runtime: &crate::TilesRuntime,
	) -> Result<Box<dyn TileSink>> {
		if destination.starts_with("sftp://") {
			#[cfg(feature = "sftp")]
			{
				let url = reqwest::Url::parse(destination)?;
				let stream = versatiles_core::io::SftpWriteStream::from_url(&url, runtime.ssh_identity())?;
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
		Ok(Box::new(Self::new(&path, tile_format, tile_compression)?))
	}
}

impl<W: Write + Send + Unpin> TarTileSink<W> {
	/// Create a new `TarTileSink` from any `Write` implementor.
	pub fn from_writer(writer: W, tile_format: TileFormat, tile_compression: TileCompression) -> Self {
		Self {
			// `new_non_terminated` avoids `Builder::new`'s `W: 'static` bound, which
			// comes from the task it spawns to terminate the archive on drop.
			// `finish()` below writes that same terminator.
			builder: Mutex::new(Builder::new_non_terminated(SyncWriteAdapter(writer))),
			tile_format,
			tile_compression,
		}
	}
}

impl<W: Write + Send + Unpin> TileSink for TarTileSink<W> {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
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

		let mut builder = self.builder.lock().expect("poisoned mutex");
		futures::executor::block_on(builder.append_data(&mut header, Path::new(&filename), blob.as_slice()))?;
		Ok(())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, _runtime: &crate::TilesRuntime) -> Result<()> {
		let mut builder = self.builder.into_inner().expect("poisoned mutex");

		// Write tiles.json metadata entry
		let meta_blob = compress(Blob::from(tilejson), &self.tile_compression)?;
		let filename = format!("tiles.json{}", self.tile_compression.as_extension());
		let mut header = Header::new_gnu();
		header.set_size(meta_blob.len());
		header.set_mode(0o644);
		futures::executor::block_on(async {
			builder
				.append_data(&mut header, Path::new(&filename), meta_blob.as_slice())
				.await?;
			builder.finish().await
		})?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TarTilesReader, TileSource, TilesRuntime};

	// `tokio::test` rather than a manually built runtime: these now also exercise
	// the claim on `SyncWriteAdapter`, since `write_tile` runs its `block_on`
	// inside a current-thread runtime, where parking would deadlock.
	#[tokio::test]
	async fn write_and_read_back() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink.tar")?;

		let sink = TarTileSink::new(&temp, TileFormat::PNG, TileCompression::Uncompressed)?;

		let coord = TileCoord::new(3, 1, 2)?;
		let blob = Blob::from(vec![0u8; 16]);
		sink.write_tile(&coord, &blob)?;

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson, &TilesRuntime::default())?;

		let reader = TarTilesReader::open(&temp).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::PNG);
		assert_eq!(reader.metadata().tile_compression(), &TileCompression::Uncompressed);
		assert_eq!(reader.tile_pyramid().await?.count_tiles(), 1);

		Ok(())
	}

	#[tokio::test]
	async fn write_multiple_tiles() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink_multi.tar")?;

		let sink = TarTileSink::new(&temp, TileFormat::WEBP, TileCompression::Brotli)?;

		for y in 0..4 {
			for x in 0..4 {
				let coord = TileCoord::new(2, x, y)?;
				#[expect(clippy::cast_possible_truncation, reason = "`x` runs 0..4")]
				let blob = Blob::from(vec![x as u8; 8]);
				sink.write_tile(&coord, &blob)?;
			}
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson, &TilesRuntime::default())?;

		let reader = TarTilesReader::open(&temp).await?;
		assert_eq!(reader.metadata().tile_format(), &TileFormat::WEBP);
		assert_eq!(reader.metadata().tile_compression(), &TileCompression::Brotli);
		assert_eq!(reader.tile_pyramid().await?.count_tiles(), 16);

		Ok(())
	}
}
