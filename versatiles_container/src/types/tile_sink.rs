use crate::{DirectoryTileSink, MBTilesTileSink, TarTileSink, TilesRuntime, VersaTilesSink};
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
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
	/// The `runtime` provides access to progress reporting and other services.
	///
	/// Uses `Box<Self>` instead of `self` for object safety.
	fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &TilesRuntime) -> Result<()>;
}

/// Wrapper that ensures each tile coordinate is written at most once.
///
/// Silently drops duplicate writes. Delegates all other operations to the inner sink.
struct DeduplicatingSink {
	inner: Box<dyn TileSink>,
	written: Mutex<HashSet<TileCoord>>,
}

impl TileSink for DeduplicatingSink {
	fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		if !self.written.lock().unwrap().insert(*coord) {
			return Ok(());
		}
		self.inner.write_tile(coord, blob)
	}

	fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &TilesRuntime) -> Result<()> {
		self.inner.finish(tilejson, runtime)
	}
}

/// Wrap a tile sink so that each coordinate is written at most once.
#[must_use]
pub fn deduplicating_tile_sink(sink: Box<dyn TileSink>) -> Box<dyn TileSink> {
	Box::new(DeduplicatingSink {
		inner: sink,
		written: Mutex::new(HashSet::new()),
	})
}

/// Open a tile sink based on the destination's file extension.
///
/// The destination can be a local path or an `sftp://` URL.
///
/// Dispatches to the appropriate sink implementation:
/// - `.tar` → [`TarTileSink`]
/// - `.mbtiles` → [`MBTilesTileSink`] (local only)
/// - `.versatiles` → [`VersaTilesSink`]
/// - directory (no extension or existing directory) → [`DirectoryTileSink`]
///
/// # Arguments
/// * `destination` — Output path or URL. Extension determines the container format.
/// * `format` — Tile format (e.g., PNG, WEBP, MVT).
/// * `compression` — Tile compression (e.g., Uncompressed, Gzip, Brotli).
/// * `runtime` — Runtime for SSH identity and other services.
///
/// # Errors
/// Returns an error if the extension is unsupported, or if the sink cannot be created.
pub fn open_tile_sink(
	destination: &str,
	format: TileFormat,
	compression: TileCompression,
	runtime: &TilesRuntime,
) -> Result<Box<dyn TileSink>> {
	// Extract extension from destination (handles both local paths and sftp:// URLs)
	let extension = if destination.starts_with("sftp://") {
		extract_extension_from_url(destination)
	} else {
		Path::new(destination)
			.extension()
			.and_then(|e| e.to_str())
			.map(str::to_ascii_lowercase)
	};

	let sink = match extension.as_deref() {
		Some("tar") => TarTileSink::open(destination, format, compression, runtime)?,
		Some("mbtiles") => MBTilesTileSink::open(destination, format, compression, runtime)?,
		Some("versatiles") => VersaTilesSink::open(destination, format, compression, runtime)?,
		_ => {
			let is_dir = !destination.starts_with("sftp://") && {
				let path = Path::new(destination);
				path.is_dir() || path.extension().is_none()
			};
			if destination.starts_with("sftp://") || is_dir {
				DirectoryTileSink::open(destination, format, compression, runtime)?
			} else {
				bail!(
					"unsupported tile sink format: .{}",
					Path::new(destination).extension().unwrap().to_string_lossy()
				)
			}
		}
	};
	Ok(deduplicating_tile_sink(sink))
}

/// Extract the file extension from an SFTP URL.
fn extract_extension_from_url(url: &str) -> Option<String> {
	let path_part = url.rsplit_once('/')?.1;
	let ext = path_part.rsplit_once('.')?.1;
	Some(ext.to_ascii_lowercase())
}
