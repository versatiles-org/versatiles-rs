use crate::{DirectoryTileSink, MBTilesTileSink, TarTileSink};
use anyhow::{Result, bail};
use std::path::Path;
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON};

/// Push-model interface for writing individual tiles to a container in any order.
///
/// Unlike [`TilesWriter`](super::TilesWriter) (which pulls from a `TileSource`), a `TileSink`
/// receives pre-compressed blobs one at a time. The caller controls the pipeline
/// and decides when to call [`finish`](TileSink::finish).
///
/// Implementations use interior mutability (`Mutex`, connection pools, etc.)
/// so that callers can share `&dyn TileSink` across threads via `Arc`.
///
/// The tile format and compression are fixed at construction time; every blob
/// passed to [`write_tile`](TileSink::write_tile) must already be encoded and compressed accordingly.
pub trait TileSink: Send + Sync {
	/// Write a single pre-compressed tile blob at the given coordinate.
	///
	/// The blob must already be encoded in the sink's configured `TileFormat`
	/// and compressed with the sink's configured `TileCompression`.
	///
	/// Implementations must be safe to call from multiple threads concurrently.
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()>;

	/// Finalize the container, writing metadata and flushing all buffers.
	///
	/// Consumes the sink to prevent use-after-close. The `tilejson` parameter
	/// provides the final accumulated metadata for the output container.
	///
	/// Uses `Box<Self>` instead of `self` for object safety.
	fn finish(self: Box<Self>, tilejson: &TileJSON) -> Result<()>;
}

/// Open a tile sink based on the output path's file extension.
///
/// Dispatches to the appropriate sink implementation:
/// - `.tar` → [`TarTileSink`]
/// - `.mbtiles` → [`MBTilesTileSink`]
/// - directory (no extension or existing directory) → [`DirectoryTileSink`]
///
/// # Arguments
/// * `path` — Output path. Extension determines the container format.
/// * `format` — Tile format (e.g., PNG, WEBP, MVT).
/// * `compression` — Tile compression (e.g., Uncompressed, Gzip, Brotli).
///
/// # Errors
/// Returns an error if the extension is unsupported, or if the sink cannot be created.
pub fn open_tile_sink(path: &Path, format: TileFormat, compression: TileCompression) -> Result<Box<dyn TileSink>> {
	match path.extension().and_then(|e| e.to_str()) {
		Some(ext) if ext.eq_ignore_ascii_case("tar") => Ok(Box::new(TarTileSink::new(path, format, compression)?)),
		Some(ext) if ext.eq_ignore_ascii_case("mbtiles") => {
			Ok(Box::new(MBTilesTileSink::new(path, format, compression)?))
		}
		_ if path.is_dir() || path.extension().is_none() => Ok(Box::new(DirectoryTileSink::new(
			path.to_path_buf(),
			format,
			compression,
		)?)),
		Some(ext) => bail!("unsupported tile sink format: .{ext}"),
		None => bail!("output path has no extension and is not a directory"),
	}
}
