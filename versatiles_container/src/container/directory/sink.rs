//! A [`TileSink`] implementation that writes tiles to a directory pyramid on disk.
//!
//! Uses the same `{z}/{x}/{y}.<format>[.<compression>]` layout as [`DirectoryWriter`](super::DirectoryWriter).
//! No locks needed — each tile writes to a unique file path.

use crate::TileSink;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress};

/// A tile sink that writes pre-compressed blobs to a directory tree.
///
/// Constructed with a base path, `TileFormat`, and `TileCompression`. Every blob
/// passed to [`write_tile`](TileSink::write_tile) must already be encoded
/// and compressed accordingly.
///
/// # Thread Safety
///
/// No internal locks — each tile coordinate maps to a unique file path,
/// so concurrent calls from different threads are safe.
pub struct DirectoryTileSink {
	base_path: PathBuf,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

impl DirectoryTileSink {
	/// Create a new `DirectoryTileSink` that writes tiles under `base_path`.
	///
	/// Creates the base directory if it doesn't exist.
	pub fn new(base_path: PathBuf, tile_format: TileFormat, tile_compression: TileCompression) -> Result<Self> {
		if !base_path.exists() {
			fs::create_dir_all(&base_path)
				.with_context(|| format!("Failed to create output directory: {}", base_path.display()))?;
		}
		Ok(Self {
			base_path,
			tile_format,
			tile_compression,
		})
	}
}

impl TileSink for DirectoryTileSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		let filename = format!(
			"{}/{}/{}{}{}",
			coord.level,
			coord.x,
			coord.y,
			self.tile_format.as_extension(),
			self.tile_compression.as_extension()
		);
		let path = self.base_path.join(filename);

		if let Some(parent) = path.parent()
			&& !parent.exists()
		{
			fs::create_dir_all(parent)?;
		}

		fs::write(&path, blob.as_slice()).with_context(|| format!("Failed to write tile to {}", path.display()))?;
		Ok(())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON) -> Result<()> {
		let meta_blob = compress(Blob::from(tilejson), self.tile_compression)?;
		let filename = format!("tiles.json{}", self.tile_compression.as_extension());
		let path = self.base_path.join(filename);
		fs::write(&path, meta_blob.as_slice())
			.with_context(|| format!("Failed to write metadata to {}", path.display()))?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::compression::decompress_gzip;

	#[test]
	fn write_and_read_back() -> Result<()> {
		let temp = assert_fs::TempDir::new()?;
		let base = temp.path().to_path_buf();

		let sink = DirectoryTileSink::new(base.clone(), TileFormat::PNG, TileCompression::Uncompressed)?;

		let coord = TileCoord::new(3, 1, 2)?;
		let blob = Blob::from(vec![0u8; 16]);
		sink.write_tile(&coord, &blob)?;

		// Verify file exists
		let tile_path = base.join("3/1/2.png");
		assert!(tile_path.exists());
		assert_eq!(fs::read(&tile_path)?, vec![0u8; 16]);

		// Finish and verify metadata
		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson)?;

		let meta_path = base.join("tiles.json");
		assert!(meta_path.exists());

		Ok(())
	}

	#[test]
	fn write_with_compression() -> Result<()> {
		let temp = assert_fs::TempDir::new()?;
		let base = temp.path().to_path_buf();

		let sink = DirectoryTileSink::new(base.clone(), TileFormat::MVT, TileCompression::Gzip)?;

		let coord = TileCoord::new(2, 3, 3)?;
		let raw = Blob::from(vec![42u8; 8]);
		let compressed = versatiles_core::compression::compress_gzip(&raw)?;
		sink.write_tile(&coord, &compressed)?;

		// Verify file exists with correct extension
		let tile_path = base.join("2/3/3.pbf.gz");
		assert!(tile_path.exists());

		// Decompress and verify content
		let read_back = Blob::from(fs::read(&tile_path)?);
		let decompressed = decompress_gzip(&read_back)?;
		assert_eq!(decompressed.as_slice(), &[42u8; 8]);

		// Finish and verify metadata has compression extension
		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		Box::new(sink).finish(&tilejson)?;

		let meta_path = base.join("tiles.json.gz");
		assert!(meta_path.exists());

		Ok(())
	}
}
