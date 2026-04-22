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
		if !self.written.lock().expect("poisoned mutex").insert(*coord) {
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
					Path::new(destination).extension().expect("extension matched above").to_string_lossy()
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

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};

	/// A mock TileSink that counts write_tile calls and records coords.
	struct MockSink {
		writes: AtomicUsize,
		coords: Mutex<Vec<TileCoord>>,
		finished: Mutex<bool>,
	}

	impl MockSink {
		fn new() -> Self {
			Self {
				writes: AtomicUsize::new(0),
				coords: Mutex::new(Vec::new()),
				finished: Mutex::new(false),
			}
		}
	}

	impl TileSink for MockSink {
		fn write_tile(&self, coord: &TileCoord, _blob: &Blob) -> Result<()> {
			self.writes.fetch_add(1, Ordering::Relaxed);
			self.coords.lock().unwrap().push(*coord);
			Ok(())
		}

		fn finish(self: Box<Self>, _tilejson: &TileJSON, _runtime: &TilesRuntime) -> Result<()> {
			*self.finished.lock().unwrap() = true;
			Ok(())
		}
	}

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	fn blob(data: &[u8]) -> Blob {
		Blob::from(data.to_vec())
	}

	// ─── extract_extension_from_url ───

	#[test]
	fn test_extract_extension_tar() {
		assert_eq!(
			extract_extension_from_url("sftp://host/path/file.tar"),
			Some("tar".to_string())
		);
	}

	#[test]
	fn test_extract_extension_versatiles() {
		assert_eq!(
			extract_extension_from_url("sftp://host/data/tiles.versatiles"),
			Some("versatiles".to_string())
		);
	}

	#[test]
	fn test_extract_extension_mbtiles() {
		assert_eq!(
			extract_extension_from_url("sftp://user:pass@host:22/out.mbtiles"),
			Some("mbtiles".to_string())
		);
	}

	#[test]
	fn test_extract_extension_uppercase() {
		assert_eq!(
			extract_extension_from_url("sftp://host/FILE.TAR"),
			Some("tar".to_string())
		);
	}

	#[test]
	fn test_extract_extension_no_extension() {
		assert_eq!(extract_extension_from_url("sftp://host/path/directory"), None);
	}

	#[test]
	fn test_extract_extension_no_path() {
		assert_eq!(extract_extension_from_url("sftp://host"), None);
	}

	#[test]
	fn test_extract_extension_trailing_slash() {
		assert_eq!(extract_extension_from_url("sftp://host/path/"), None);
	}

	#[test]
	fn test_extract_extension_dotfile() {
		assert_eq!(
			extract_extension_from_url("sftp://host/.hidden"),
			Some("hidden".to_string())
		);
	}

	// ─── deduplicating_tile_sink ───

	#[test]
	fn test_dedup_sink_passes_first_write() -> Result<()> {
		let mock = MockSink::new();
		let sink = deduplicating_tile_sink(Box::new(mock));
		let c = coord(5, 1, 2);
		sink.write_tile(&c, &blob(b"data"))?;
		// Can't inspect mock directly after wrapping, but it should not error
		Ok(())
	}

	#[test]
	fn test_dedup_sink_drops_duplicate_writes() -> Result<()> {
		// We need a way to observe writes. Use Arc<MockSink> pattern via shared state.
		let write_count = std::sync::Arc::new(AtomicUsize::new(0));
		let count_clone = write_count.clone();

		struct CountingSink {
			count: std::sync::Arc<AtomicUsize>,
		}
		impl TileSink for CountingSink {
			fn write_tile(&self, _coord: &TileCoord, _blob: &Blob) -> Result<()> {
				self.count.fetch_add(1, Ordering::Relaxed);
				Ok(())
			}
			fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				Ok(())
			}
		}

		let sink = deduplicating_tile_sink(Box::new(CountingSink { count: count_clone }));
		let c = coord(3, 0, 0);
		sink.write_tile(&c, &blob(b"first"))?;
		sink.write_tile(&c, &blob(b"second"))?;
		sink.write_tile(&c, &blob(b"third"))?;
		assert_eq!(write_count.load(Ordering::Relaxed), 1);
		Ok(())
	}

	#[test]
	fn test_dedup_sink_allows_different_coords() -> Result<()> {
		let write_count = std::sync::Arc::new(AtomicUsize::new(0));
		let count_clone = write_count.clone();

		struct CountingSink {
			count: std::sync::Arc<AtomicUsize>,
		}
		impl TileSink for CountingSink {
			fn write_tile(&self, _coord: &TileCoord, _blob: &Blob) -> Result<()> {
				self.count.fetch_add(1, Ordering::Relaxed);
				Ok(())
			}
			fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				Ok(())
			}
		}

		let sink = deduplicating_tile_sink(Box::new(CountingSink { count: count_clone }));
		sink.write_tile(&coord(0, 0, 0), &blob(b"a"))?;
		sink.write_tile(&coord(1, 0, 0), &blob(b"b"))?;
		sink.write_tile(&coord(1, 1, 0), &blob(b"c"))?;
		assert_eq!(write_count.load(Ordering::Relaxed), 3);
		Ok(())
	}

	#[test]
	fn test_dedup_sink_finish_delegates() -> Result<()> {
		let finished = std::sync::Arc::new(Mutex::new(false));
		let finished_clone = finished.clone();

		struct FinishSink {
			finished: std::sync::Arc<Mutex<bool>>,
		}
		impl TileSink for FinishSink {
			fn write_tile(&self, _: &TileCoord, _: &Blob) -> Result<()> {
				Ok(())
			}
			fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				*self.finished.lock().unwrap() = true;
				Ok(())
			}
		}

		let sink = deduplicating_tile_sink(Box::new(FinishSink {
			finished: finished_clone,
		}));
		let runtime = TilesRuntime::new();
		sink.finish(&TileJSON::default(), &runtime)?;
		assert!(*finished.lock().unwrap());
		Ok(())
	}

	// ─── open_tile_sink ───

	#[test]
	fn test_open_tile_sink_tar() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.tar");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		// Should succeed and be writable
		sink.write_tile(&coord(0, 0, 0), &blob(b"data"))?;
		Ok(())
	}

	#[test]
	fn test_open_tile_sink_versatiles() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.versatiles");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		sink.write_tile(&coord(0, 0, 0), &blob(b"tile"))?;
		Ok(())
	}

	#[test]
	fn test_open_tile_sink_directory() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let out = dir.path().join("tiles");
		std::fs::create_dir(&out)?;
		let runtime = TilesRuntime::new();
		let _sink = open_tile_sink(
			out.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		Ok(())
	}

	#[test]
	fn test_open_tile_sink_no_extension_creates_directory() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let out = dir.path().join("output_tiles");
		let runtime = TilesRuntime::new();
		let _sink = open_tile_sink(
			out.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		Ok(())
	}

	#[test]
	fn test_open_tile_sink_unsupported_extension() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("out.xyz");
		let runtime = TilesRuntime::new();
		let result = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		);
		let err = result.err().expect("should fail for unsupported extension");
		assert!(err.to_string().contains("unsupported"));
	}

	#[test]
	fn test_open_tile_sink_deduplicates() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.tar");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		// Writing the same coord twice should silently drop the second
		let c = coord(5, 3, 4);
		sink.write_tile(&c, &blob(b"first"))?;
		sink.write_tile(&c, &blob(b"second"))?; // should be dropped, no error
		Ok(())
	}

	#[test]
	fn test_open_tile_sink_mbtiles() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.mbtiles");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)?;
		sink.write_tile(&coord(0, 0, 0), &blob(b"tile"))?;
		Ok(())
	}
}
