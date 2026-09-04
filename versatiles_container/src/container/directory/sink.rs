//! A [`TileSink`] implementation that writes tiles to a directory pyramid on disk.
//!
//! Uses the same `{z}/{x}/{y}.<format>[.<compression>]` layout as [`DirectoryWriter`](super::DirectoryWriter).
//! Supports both local paths and `sftp://` URLs as output destinations.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress};

use crate::TileSink;

/// Backend abstraction for writing files (local or SFTP).
enum Backend {
	Local {
		base_path: PathBuf,
	},
	// Boxed: an `SftpFileSystem` carries a live SSH session and an SFTP channel,
	// which makes it an order of magnitude larger than the local variant, and
	// every `Backend` — local ones included — would otherwise be sized for it.
	#[cfg(feature = "sftp")]
	Sftp(Box<std::sync::Mutex<versatiles_core::io::SftpFileSystem>>),
}

impl Backend {
	fn write_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
		match self {
			Self::Local { base_path } => {
				let path = base_path.join(rel_path);
				if let Some(parent) = path.parent()
					&& !parent.exists()
				{
					fs::create_dir_all(parent)?;
				}
				fs::write(&path, data).with_context(|| format!("Failed to write to {}", path.display()))
			}
			#[cfg(feature = "sftp")]
			Self::Sftp(fs) => fs.lock().expect("poisoned mutex").write_file(rel_path, data),
		}
	}
}

// The sink is shared across the tasks writing a directory tree, so `Backend`
// has to be thread-safe. Both variants earn it on their own: `Local` holds only
// a `PathBuf`, and `Sftp` holds its file system behind a `Mutex`. Asserting it
// rather than hand-implementing it makes a change to either variant a compile
// error instead of a silent one.
const _: fn() = || {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<Backend>();
};

/// A tile sink that writes pre-compressed blobs to a directory tree.
///
/// Supports both local filesystem paths and `sftp://` URLs.
pub struct DirectoryTileSink {
	tile_format: TileFormat,
	tile_compression: TileCompression,
	backend: Backend,
}

impl DirectoryTileSink {
	/// Open a directory tile sink from a destination string (local path or `sftp://` URL).
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
				let sftp_fs = versatiles_core::io::SftpFileSystem::from_url(&url, runtime.ssh_identity())?;
				return Ok(Box::new(Self {
					tile_format,
					tile_compression,
					backend: Backend::Sftp(Box::new(std::sync::Mutex::new(sftp_fs))),
				}));
			}
			#[cfg(not(feature = "sftp"))]
			{
				let _ = runtime;
				anyhow::bail!("SFTP support requires the 'sftp' feature");
			}
		}

		let _ = runtime;
		let base_path = std::env::current_dir()?.join(destination);
		if !base_path.exists() {
			fs::create_dir_all(&base_path)
				.with_context(|| format!("Failed to create output directory: {}", base_path.display()))?;
		}

		Ok(Box::new(Self {
			tile_format,
			tile_compression,
			backend: Backend::Local { base_path },
		}))
	}
}

impl TileSink for DirectoryTileSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		let rel_path = format!(
			"{}/{}/{}{}{}",
			coord.level,
			coord.x,
			coord.y,
			self.tile_format.as_extension(),
			self.tile_compression.as_extension()
		);
		self.backend.write_file(&rel_path, blob.as_slice())
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, _runtime: &crate::TilesRuntime) -> Result<()> {
		let meta_blob = compress(Blob::from(tilejson), &self.tile_compression)?;
		let filename = format!("tiles.json{}", self.tile_compression.as_extension());
		self.backend.write_file(&filename, meta_blob.as_slice())
	}
}

#[cfg(test)]
mod tests {
	use versatiles_core::compression::decompress_gzip;

	use super::*;
	use crate::TilesRuntime;

	#[test]
	fn write_and_read_back() -> Result<()> {
		let temp = assert_fs::TempDir::new()?;
		let base = temp.path().to_path_buf();
		let runtime = TilesRuntime::default();

		let sink = DirectoryTileSink::open(
			base.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;

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
		Box::new(sink).finish(&tilejson, &runtime)?;

		let meta_path = base.join("tiles.json");
		assert!(meta_path.exists());

		Ok(())
	}

	#[test]
	fn write_with_compression() -> Result<()> {
		let temp = assert_fs::TempDir::new()?;
		let base = temp.path().to_path_buf();
		let runtime = TilesRuntime::default();

		let sink = DirectoryTileSink::open(base.to_str().unwrap(), TileFormat::MVT, TileCompression::Gzip, &runtime)?;

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
		Box::new(sink).finish(&tilejson, &runtime)?;

		let meta_path = base.join("tiles.json.gz");
		assert!(meta_path.exists());

		Ok(())
	}
}
