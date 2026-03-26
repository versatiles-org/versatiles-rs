//! A [`TileSink`] implementation that writes tiles to a directory pyramid on disk.
//!
//! Uses the same `{z}/{x}/{y}.<format>[.<compression>]` layout as [`DirectoryWriter`](super::DirectoryWriter).
//! Supports both local paths and `sftp://` URLs as output destinations.

use crate::TileSink;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON, compression::compress};

/// Backend abstraction for writing files (local or SFTP).
enum Backend {
	Local {
		base_path: PathBuf,
	},
	#[cfg(feature = "ssh2")]
	Sftp(std::sync::Mutex<versatiles_core::io::SftpFileSystem>),
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
			#[cfg(feature = "ssh2")]
			Self::Sftp(fs) => fs.lock().unwrap().write_file(rel_path, data),
		}
	}
}

// Backend is Send+Sync because Local uses only PathBuf (Send+Sync) and
// Sftp wraps SftpFileSystem (Send+Sync) in a Mutex.
unsafe impl Send for Backend {}
unsafe impl Sync for Backend {}

/// A tile sink that writes pre-compressed blobs to a directory tree.
///
/// Supports both local filesystem paths and `sftp://` URLs.
pub struct DirectoryTileSink {
	tile_format: TileFormat,
	tile_compression: TileCompression,
	backend: Backend,
	written: Mutex<HashSet<TileCoord>>,
}

impl DirectoryTileSink {
	/// Open a directory tile sink from a destination string (local path or `sftp://` URL).
	pub fn open(
		destination: &str,
		tile_format: TileFormat,
		tile_compression: TileCompression,
		runtime: &crate::TilesRuntime,
	) -> Result<Self> {
		if destination.starts_with("sftp://") {
			#[cfg(feature = "ssh2")]
			{
				let url = reqwest::Url::parse(destination)?;
				let sftp_fs = versatiles_core::io::SftpFileSystem::from_url(&url, runtime.ssh_identity())?;
				return Ok(Self {
					tile_format,
					tile_compression,
					backend: Backend::Sftp(std::sync::Mutex::new(sftp_fs)),
					written: Mutex::new(HashSet::new()),
				});
			}
			#[cfg(not(feature = "ssh2"))]
			{
				let _ = runtime;
				anyhow::bail!("SFTP support requires the 'ssh2' feature");
			}
		}

		let _ = runtime;
		let base_path = std::env::current_dir()?.join(destination);
		if !base_path.exists() {
			fs::create_dir_all(&base_path)
				.with_context(|| format!("Failed to create output directory: {}", base_path.display()))?;
		}

		Ok(Self {
			tile_format,
			tile_compression,
			backend: Backend::Local { base_path },
			written: Mutex::new(HashSet::new()),
		})
	}
}

impl TileSink for DirectoryTileSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		if !self.written.lock().unwrap().insert(*coord) {
			return Ok(());
		}
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
		let meta_blob = compress(Blob::from(tilejson), self.tile_compression)?;
		let filename = format!("tiles.json{}", self.tile_compression.as_extension());
		self.backend.write_file(&filename, meta_blob.as_slice())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TilesRuntime;
	use versatiles_core::compression::decompress_gzip;

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
