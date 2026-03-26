//! A [`TileSink`] implementation that appends tiles to a `.tar` archive.
//!
//! Uses the same `{z}/{x}/{y}.<format>[.<compression>]` layout as [`TarTilesWriter`](super::TarTilesWriter).
//! Thread-safe via an internal `Mutex` around the `tar::Builder`.
//!
//! Supports both local paths and `sftp://` URLs as output destinations.

use crate::TileSink;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use tar::{Builder, Header};
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress};

/// A tile sink that writes pre-compressed blobs into a `.tar` archive.
///
/// Constructed with a fixed `TileFormat` and `TileCompression`. Every blob
/// passed to [`write_tile`](TileSink::write_tile) must already be encoded
/// and compressed accordingly.
///
/// # Thread Safety
///
/// Uses a `std::sync::Mutex` around the inner `tar::Builder` so that
/// multiple threads can call `write_tile` concurrently (e.g. from
/// `spawn_blocking` tasks).
pub struct TarTileSink<W: Write + Send> {
	builder: Mutex<Builder<W>>,
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
			#[cfg(feature = "ssh2")]
			{
				let url = reqwest::Url::parse(destination)?;
				let stream = versatiles_core::io::SftpWriteStream::from_url(&url, runtime.ssh_identity())?;
				return Ok(Box::new(TarTileSink::from_writer(
					BufWriter::new(stream),
					tile_format,
					tile_compression,
				)));
			}
			#[cfg(not(feature = "ssh2"))]
			{
				let _ = runtime;
				anyhow::bail!("SFTP support requires the 'ssh2' feature");
			}
		}

		let _ = runtime;
		let path = std::env::current_dir()?.join(destination);
		Ok(Box::new(Self::new(&path, tile_format, tile_compression)?))
	}
}

impl<W: Write + Send> TarTileSink<W> {
	/// Create a new `TarTileSink` from any `Write` implementor.
	pub fn from_writer(writer: W, tile_format: TileFormat, tile_compression: TileCompression) -> Self {
		Self {
			builder: Mutex::new(Builder::new(writer)),
			tile_format,
			tile_compression,
		}
	}
}

impl<W: Write + Send> TileSink for TarTileSink<W> {
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

		self
			.builder
			.lock()
			.unwrap()
			.append_data(&mut header, Path::new(&filename), blob.as_slice())?;
		Ok(())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, _runtime: &crate::TilesRuntime) -> Result<()> {
		let mut builder = self.builder.into_inner().unwrap();

		// Write tiles.json metadata entry
		let meta_blob = compress(Blob::from(tilejson), self.tile_compression)?;
		let filename = format!("tiles.json{}", self.tile_compression.as_extension());
		let mut header = Header::new_gnu();
		header.set_size(meta_blob.len());
		header.set_mode(0o644);
		builder.append_data(&mut header, Path::new(&filename), meta_blob.as_slice())?;

		builder.finish()?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TarTilesReader, TileSource, TilesRuntime};

	#[test]
	fn write_and_read_back() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink.tar")?;

		let sink = TarTileSink::new(&temp, TileFormat::PNG, TileCompression::Uncompressed)?;

		let coord = TileCoord::new(3, 1, 2)?;
		let blob = Blob::from(vec![0u8; 16]);
		sink.write_tile(&coord, &blob)?;

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson, &TilesRuntime::default())?;

		let reader = TarTilesReader::open(&temp)?;
		assert_eq!(reader.metadata().tile_format, TileFormat::PNG);
		assert_eq!(reader.metadata().tile_compression, TileCompression::Uncompressed);
		assert_eq!(reader.metadata().bbox_pyramid.count_tiles(), 1);

		Ok(())
	}

	#[test]
	fn write_multiple_tiles() -> Result<()> {
		let temp = assert_fs::NamedTempFile::new("test_sink_multi.tar")?;

		let sink = TarTileSink::new(&temp, TileFormat::WEBP, TileCompression::Brotli)?;

		for y in 0..4 {
			for x in 0..4 {
				let coord = TileCoord::new(2, x, y)?;
				#[allow(clippy::cast_possible_truncation)]
				let blob = Blob::from(vec![x as u8; 8]);
				sink.write_tile(&coord, &blob)?;
			}
		}

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson, &TilesRuntime::default())?;

		let reader = TarTilesReader::open(&temp)?;
		assert_eq!(reader.metadata().tile_format, TileFormat::WEBP);
		assert_eq!(reader.metadata().tile_compression, TileCompression::Brotli);
		assert_eq!(reader.metadata().bbox_pyramid.count_tiles(), 16);

		Ok(())
	}
}
