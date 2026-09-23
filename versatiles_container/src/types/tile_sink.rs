use std::{collections::HashSet, path::Path, sync::Mutex};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat, TileJSON};

use crate::{
	DirectoryTileSink, MBTilesTileSink, TarTileSink, TilesRuntime, VersaTilesSink, types::staged_output::StagedOutput,
};

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
///
/// Takes tiles one at a time from several threads; [`TilesWriter`](crate::TilesWriter)
/// is the whole-source counterpart, and not every format supports both — see
/// the [crate documentation](crate) for the grid and the one exception.
#[async_trait]
pub trait TileSink: Send + Sync {
	/// Write a single pre-compressed tile blob at the given coordinate.
	///
	/// The blob must already be encoded in the sink's configured `TileFormat`
	/// and compressed with the sink's configured `TileCompression`.
	///
	/// Implementations must be safe to call from multiple threads concurrently.
	async fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()>;

	/// Finalize the container, writing metadata and flushing all buffers.
	///
	/// Consumes the sink to prevent use-after-close. The `tilejson` parameter
	/// provides the final accumulated metadata for the output container.
	/// The `runtime` provides access to progress reporting and other services.
	///
	/// Uses `Box<Self>` instead of `self` for object safety.
	///
	/// # Known gap: sinks write in place
	///
	/// [`TilesWriter`](crate::TilesWriter) implementations are handed a staging
	/// path and the caller publishes the result, so a destination only ever holds
	/// a complete output. **Sinks do not work that way yet.** They open the
	/// destination directly, which means a run that fails part-way leaves a
	/// partial container under the name the user asked for — and
	/// [`MBTilesTileSink`](crate::MBTilesTileSink) removes an existing file
	/// before it starts, so a failure costs the previous output.
	///
	/// [`open_tile_sink`] is the choke point where the same treatment belongs: a
	/// wrapper holding the staging location and publishing it in `finish`, the
	/// way [`deduplicating_tile_sink`] wraps for its own concern. Until then,
	/// `versatiles mosaic assemble` is the user-facing path that does not get the
	/// guarantee.
	async fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &TilesRuntime) -> Result<()>;
}

/// Wrapper that ensures each tile coordinate is written at most once.
///
/// Silently drops duplicate writes. Delegates all other operations to the inner sink.
struct DeduplicatingSink {
	inner: Box<dyn TileSink>,
	written: Mutex<HashSet<TileCoord>>,
}

#[async_trait]
impl TileSink for DeduplicatingSink {
	async fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		// The lock is released before the await: it guards only the seen-set, and
		// holding a `std::sync::Mutex` across an await point would be unsound.
		{
			if !self.written.lock().expect("poisoned mutex").insert(*coord) {
				return Ok(());
			}
		}
		self.inner.write_tile(coord, blob).await
	}

	async fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &TilesRuntime) -> Result<()> {
		self.inner.finish(tilejson, runtime).await
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

/// Whether a local output path names a directory of tiles rather than a container file.
///
/// The single rule both write paths dispatch on — [`open_tile_sink`] here and
/// [`ContainerRegistry::write_to_path`](crate::ContainerRegistry::write_to_path).
/// They used to answer it differently, so `convert in.versatiles out/` worked
/// through one and failed through the other with *"file extension '' unknown"*
/// (issue #245).
///
/// A path that does not exist yet still counts as a directory when it has no
/// extension: no container format could be picked from an empty extension, and
/// the writer creates the directory on the way. The `!exists` guard keeps an
/// *existing* extensionless file out — that is a file, and writing tiles into it
/// has to fail.
#[must_use]
pub(crate) fn destination_is_directory(path: &Path) -> bool {
	if path.is_dir() {
		return true;
	}
	if path.exists() {
		return false;
	}
	// A trailing separator says "directory" outright, whatever the name looks
	// like. `out.dir/` is a directory called `out.dir`, and reading `.dir` off it
	// as a container extension produced "file extension 'dir' unknown" for a
	// destination that could not have meant anything else.
	ends_with_separator(path) || path.extension().is_none()
}

/// Whether `path` was written with a trailing path separator.
///
/// `Path` keeps the string as given, so the separator survives here even though
/// `components()` and `extension()` both look past it.
fn ends_with_separator(path: &Path) -> bool {
	path
		.as_os_str()
		.as_encoded_bytes()
		.last()
		.is_some_and(|byte| *byte == b'/' || (cfg!(windows) && *byte == b'\\'))
}

/// Which container a destination names, decided before anything is created.
///
/// Separated from construction so a bad extension fails without a staging
/// location having been made for it.
enum SinkKind {
	Tar,
	MBTiles,
	VersaTiles,
	Directory,
}

/// Where a sink's finished output goes.
enum SinkPublish {
	Local(StagedOutput),
	#[cfg(feature = "sftp")]
	Remote {
		url: reqwest::Url,
		identity: Option<std::path::PathBuf>,
		staging: String,
		destination: String,
	},
}

/// Wraps a sink so its output is built beside the destination and moved into
/// place only once it is finished.
///
/// Sinks open their output and write to it over the lifetime of a run, so they
/// cannot be handed a finished file the way a [`TilesWriter`](crate::TilesWriter)
/// can. The staging location travels with the sink instead and is published by
/// `finish` — which is the one moment the whole container is known to be
/// complete.
///
/// Without this, `versatiles mosaic assemble` wrote straight to the
/// destination: a failure left a partial container under the name the user
/// asked for, and for MBTiles it had already deleted the previous one.
struct StagedSink {
	inner: Box<dyn TileSink>,
	publish: SinkPublish,
}

#[async_trait]
impl TileSink for StagedSink {
	async fn write_tile(&self, coord: &TileCoord, blob: &Blob) -> Result<()> {
		self.inner.write_tile(coord, blob).await
	}

	async fn finish(self: Box<Self>, tilejson: &TileJSON, runtime: &TilesRuntime) -> Result<()> {
		let Self { inner, publish } = *self;

		// On an error here `publish` drops, and a local staging location is
		// removed with it. Nothing has touched the destination.
		inner.finish(tilejson, runtime).await?;

		match publish {
			SinkPublish::Local(staged) => {
				if runtime.had_errors() {
					let kept = staged.publish_as_incomplete()?;
					bail!(
						"assembly completed with {} read error(s); the destination was left untouched \
						 and the incomplete output kept at {kept:?}",
						runtime.error_count()
					);
				}
				staged.publish()
			}
			#[cfg(feature = "sftp")]
			SinkPublish::Remote {
				url,
				identity,
				staging,
				destination,
			} => {
				use versatiles_core::io::sftp_utils;
				let session = sftp_utils::open_session(&url, identity.as_deref()).await?;
				let sftp = sftp_utils::open_sftp(&session).await?;
				sftp_utils::publish_remote(&sftp, &staging, &destination).await
			}
		}
	}
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
pub async fn open_tile_sink(
	destination: &str,
	format: TileFormat,
	compression: TileCompression,
	runtime: &TilesRuntime,
) -> Result<Box<dyn TileSink>> {
	let kind = classify_destination(destination)?;

	// Staged before anything is opened, and always derived from the *destination*
	// — the format was already decided above, so the staging name's `.tmp`
	// extension never reaches the dispatch.
	let (publish, staging) = stage_destination(destination, runtime)?;

	if matches!(kind, SinkKind::Directory) && !destination.starts_with("sftp://") {
		// The sinks create each tile's parent on the way; this turns an
		// unwritable destination into an error naming it, and makes an empty
		// assembly still produce a directory.
		std::fs::create_dir_all(&staging).with_context(|| format!("Failed to create output directory {staging:?}"))?;
	}

	let sink = match kind {
		SinkKind::Tar => TarTileSink::open(&staging, format, compression, runtime).await?,
		SinkKind::MBTiles => MBTilesTileSink::open(&staging, format, compression, runtime)?,
		SinkKind::VersaTiles => VersaTilesSink::open(&staging, format, compression, runtime)?,
		SinkKind::Directory => DirectoryTileSink::open(&staging, format, compression, runtime).await?,
	};

	Ok(Box::new(StagedSink {
		inner: deduplicating_tile_sink(sink),
		publish,
	}))
}

/// Which container `destination` names, or why none of them fits.
///
/// Runs before anything is created, so a destination that cannot be written
/// fails without leaving a staging location behind.
fn classify_destination(destination: &str) -> Result<SinkKind> {
	let is_remote = destination.starts_with("sftp://");
	let extension = if is_remote {
		extract_extension_from_url(destination)
	} else {
		Path::new(destination)
			.extension()
			.and_then(|e| e.to_str())
			.map(str::to_ascii_lowercase)
	};

	Ok(match extension.as_deref() {
		Some("tar") => SinkKind::Tar,
		Some("mbtiles") => SinkKind::MBTiles,
		Some("versatiles") => SinkKind::VersaTiles,
		_ => {
			if is_remote || destination_is_directory(Path::new(destination)) {
				SinkKind::Directory
			} else if let Some(extension) = extension.as_deref() {
				// PMTiles is the one format this workspace can write but not write
				// *incrementally*. A sink accepts tiles in whatever order the threads
				// producing them finish, while a clustered PMTiles archive needs them
				// ordered by Hilbert index — which is only knowable once they are all
				// in. `PMTilesWriter` solves that by reordering through a temporary
				// file, an option a sink does not have. So the answer is not "this
				// format is unsupported" but "use the command that writes it whole".
				if extension == "pmtiles" {
					bail!(
						"cannot write .pmtiles here: a PMTiles archive orders its tiles, which is only possible once every \
						 tile is known, so it cannot be written incrementally. Use `versatiles convert` to produce PMTiles, \
						 or write .versatiles, .mbtiles, .tar or a directory instead"
					)
				}
				bail!("unsupported tile sink format: .{extension}")
			} else {
				bail!(
					"cannot write tiles to {destination:?}: it is an existing file with no extension, so it names neither a container format nor a directory"
				)
			}
		}
	})
}

/// Picks where the sink actually writes, and how that gets published.
fn stage_destination(destination: &str, runtime: &TilesRuntime) -> Result<(SinkPublish, String)> {
	#[cfg(feature = "sftp")]
	if destination.starts_with("sftp://") {
		use versatiles_core::io::sftp_utils;

		let url = reqwest::Url::parse(destination).map_err(|e| anyhow::anyhow!("invalid SFTP URL: {e}"))?;
		let remote_path = sftp_utils::remote_path(&url);
		let staging_path = sftp_utils::staging_remote_path(&remote_path);

		let mut staging_url = url.clone();
		staging_url.set_path(&staging_path);

		return Ok((
			SinkPublish::Remote {
				url,
				identity: runtime.ssh_identity().map(Path::to_path_buf),
				staging: staging_path,
				destination: remote_path,
			},
			staging_url.to_string(),
		));
	}

	let staged = StagedOutput::create(Path::new(destination), runtime.force())?;
	let staging = staged
		.path()
		.to_str()
		.ok_or_else(|| anyhow::anyhow!("{destination:?} is not valid UTF-8"))?
		.to_string();
	Ok((SinkPublish::Local(staged), staging))
}

/// Extract the file extension from an SFTP URL.
fn extract_extension_from_url(url: &str) -> Option<String> {
	let path_part = url.rsplit_once('/')?.1;
	let ext = path_part.rsplit_once('.')?.1;
	Some(ext.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicUsize, Ordering};

	use super::*;

	/// The data-loss path this wrapper exists for: `mosaic assemble` into an
	/// existing `.mbtiles` used to delete it before writing a byte.
	#[tokio::test]
	async fn an_abandoned_sink_leaves_the_previous_output_untouched() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		let destination = dir.path().join("out.mbtiles");
		std::fs::write(&destination, b"the previous assembly")?;
		let runtime = TilesRuntime::new_silent();

		{
			let sink = open_tile_sink(
				destination.to_str().unwrap(),
				TileFormat::PNG,
				TileCompression::Uncompressed,
				&runtime,
			)
			.await?;
			sink
				.write_tile(&TileCoord::new(3, 1, 2)?, &Blob::from(vec![0u8; 16]))
				.await?;
			// Dropped without `finish`, which is what a failed run does.
		}

		assert_eq!(
			std::fs::read(&destination)?,
			b"the previous assembly",
			"an abandoned assembly must not cost the user their existing output"
		);
		assert_eq!(
			std::fs::read_dir(dir.path())?.count(),
			1,
			"no staging database may be left beside it"
		);
		Ok(())
	}

	/// And a finished one publishes, leaving nothing behind.
	#[tokio::test]
	async fn a_finished_sink_publishes_and_cleans_up() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		let destination = dir.path().join("out.mbtiles");
		std::fs::write(&destination, b"the previous assembly")?;
		let runtime = TilesRuntime::new_silent();

		let sink = open_tile_sink(
			destination.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		sink
			.write_tile(&TileCoord::new(3, 1, 2)?, &Blob::from(vec![0u8; 16]))
			.await?;

		let mut tilejson = TileJSON::default();
		tilejson.set_string("tilejson", "3.0.0")?;
		tilejson.set_zoom_min(3);
		tilejson.set_zoom_max(3);
		sink.finish(&tilejson, &runtime).await?;

		assert_ne!(
			std::fs::read(&destination)?,
			b"the previous assembly",
			"it was replaced"
		);
		assert_eq!(std::fs::read_dir(dir.path())?.count(), 1, "no staging left");
		crate::MBTilesReader::open(&destination, TilesRuntime::new_silent())?;
		Ok(())
	}

	/// The merge bug, in the sink family. A re-assembly must not leave tiles from
	/// the previous run mixed in with the new ones.
	#[tokio::test]
	async fn a_directory_sink_replaces_rather_than_merges() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		let destination = dir.path().join("tiles");
		std::fs::create_dir_all(destination.join("9/1"))?;
		std::fs::write(destination.join("9/1/1.png"), b"a tile from a bigger pyramid")?;

		let runtime = TilesRuntime::new_silent();
		runtime.set_force(true);

		let sink = open_tile_sink(
			destination.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		sink
			.write_tile(&TileCoord::new(3, 1, 2)?, &Blob::from(vec![0u8; 16]))
			.await?;
		sink.finish(&TileJSON::default(), &runtime).await?;

		assert!(
			!destination.join("9").exists(),
			"a zoom level from the previous assembly survived into the new output"
		);
		Ok(())
	}

	/// The `--force` guard reaches sinks too.
	#[tokio::test]
	async fn a_non_empty_directory_sink_is_refused_without_force() -> Result<()> {
		let dir = assert_fs::TempDir::new()?;
		let destination = dir.path().join("not-ours");
		std::fs::create_dir_all(&destination)?;
		std::fs::write(destination.join("thesis.txt"), b"three years of work")?;

		let runtime = TilesRuntime::new_silent();
		// `Box<dyn TileSink>` is not `Debug`, so `unwrap_err` is unavailable.
		let error = match open_tile_sink(
			destination.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await
		{
			Ok(_) => panic!("a non-empty directory must be refused without --force"),
			Err(e) => e.to_string(),
		};

		assert!(error.contains("--force"), "got: {error}");
		assert!(destination.join("thesis.txt").exists());
		Ok(())
	}

	/// PMTiles cannot be a sink, and the error has to say why rather than calling
	/// the format unsupported — `versatiles convert` writes it perfectly well.
	#[tokio::test]
	async fn a_pmtiles_destination_points_at_convert() {
		let temp = assert_fs::TempDir::new().unwrap();
		let destination = temp.path().join("mosaic.pmtiles");

		// `Box<dyn TileSink>` is not `Debug`, so `expect_err` is unavailable.
		let message = match open_tile_sink(
			destination.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&TilesRuntime::default(),
		)
		.await
		{
			Ok(_) => panic!("a pmtiles sink should not open"),
			Err(error) => format!("{error:#}"),
		};
		assert!(
			message.contains("versatiles convert"),
			"should name the way out: {message}"
		);
		assert!(
			!message.contains("unsupported tile sink format"),
			"the format is supported, just not incrementally: {message}"
		);

		// An extension nothing writes still gets the plain message.
		let unknown = temp.path().join("mosaic.sqlite3");
		let message = match open_tile_sink(
			unknown.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&TilesRuntime::default(),
		)
		.await
		{
			Ok(_) => panic!("an unknown extension should not open"),
			Err(error) => format!("{error:#}"),
		};
		assert!(
			message.contains("unsupported tile sink format"),
			"unknown extensions keep the plain message: {message}"
		);
	}

	/// A destination written with a trailing separator is a directory, whatever
	/// its name contains. `out.dir/` used to be read as a container with an
	/// unknown `.dir` extension and refused.
	#[test]
	fn a_trailing_separator_means_directory() {
		let temp = assert_fs::TempDir::new().unwrap();
		let base = temp.path();

		// Nothing exists yet: the trailing slash is the only signal.
		assert!(destination_is_directory(&base.join("tiles.dir/")));
		assert!(destination_is_directory(&base.join("tiles/")));
		// Without the separator, a known-looking extension still names a file.
		assert!(!destination_is_directory(&base.join("tiles.dir")));
		assert!(!destination_is_directory(&base.join("tiles.versatiles")));
		// An extensionless path is still a directory, as before.
		assert!(destination_is_directory(&base.join("tiles")));

		// An existing file is a file, trailing separator or not.
		std::fs::write(base.join("real.file"), b"x").unwrap();
		assert!(!destination_is_directory(&base.join("real.file")));

		// An existing directory is a directory, trailing separator or not.
		std::fs::create_dir(base.join("real.dir")).unwrap();
		assert!(destination_is_directory(&base.join("real.dir")));
	}

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

	#[async_trait]
	impl TileSink for MockSink {
		async fn write_tile(&self, coord: &TileCoord, _blob: &Blob) -> Result<()> {
			self.writes.fetch_add(1, Ordering::Relaxed);
			self.coords.lock().unwrap().push(*coord);
			Ok(())
		}

		async fn finish(self: Box<Self>, _tilejson: &TileJSON, _runtime: &TilesRuntime) -> Result<()> {
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

	#[tokio::test]
	async fn test_dedup_sink_passes_first_write() -> Result<()> {
		let mock = MockSink::new();
		let sink = deduplicating_tile_sink(Box::new(mock));
		let c = coord(5, 1, 2);
		sink.write_tile(&c, &blob(b"data")).await?;
		// Can't inspect mock directly after wrapping, but it should not error
		Ok(())
	}

	#[tokio::test]
	async fn test_dedup_sink_drops_duplicate_writes() -> Result<()> {
		// We need a way to observe writes. Use Arc<MockSink> pattern via shared state.
		let write_count = std::sync::Arc::new(AtomicUsize::new(0));
		let count_clone = write_count.clone();

		struct CountingSink {
			count: std::sync::Arc<AtomicUsize>,
		}
		#[async_trait]
		impl TileSink for CountingSink {
			async fn write_tile(&self, _coord: &TileCoord, _blob: &Blob) -> Result<()> {
				self.count.fetch_add(1, Ordering::Relaxed);
				Ok(())
			}
			async fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				Ok(())
			}
		}

		let sink = deduplicating_tile_sink(Box::new(CountingSink { count: count_clone }));
		let c = coord(3, 0, 0);
		sink.write_tile(&c, &blob(b"first")).await?;
		sink.write_tile(&c, &blob(b"second")).await?;
		sink.write_tile(&c, &blob(b"third")).await?;
		assert_eq!(write_count.load(Ordering::Relaxed), 1);
		Ok(())
	}

	#[tokio::test]
	async fn test_dedup_sink_allows_different_coords() -> Result<()> {
		let write_count = std::sync::Arc::new(AtomicUsize::new(0));
		let count_clone = write_count.clone();

		struct CountingSink {
			count: std::sync::Arc<AtomicUsize>,
		}
		#[async_trait]
		impl TileSink for CountingSink {
			async fn write_tile(&self, _coord: &TileCoord, _blob: &Blob) -> Result<()> {
				self.count.fetch_add(1, Ordering::Relaxed);
				Ok(())
			}
			async fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				Ok(())
			}
		}

		let sink = deduplicating_tile_sink(Box::new(CountingSink { count: count_clone }));
		sink.write_tile(&coord(0, 0, 0), &blob(b"a")).await?;
		sink.write_tile(&coord(1, 0, 0), &blob(b"b")).await?;
		sink.write_tile(&coord(1, 1, 0), &blob(b"c")).await?;
		assert_eq!(write_count.load(Ordering::Relaxed), 3);
		Ok(())
	}

	#[tokio::test]
	async fn test_dedup_sink_finish_delegates() -> Result<()> {
		let finished = std::sync::Arc::new(Mutex::new(false));
		let finished_clone = finished.clone();

		struct FinishSink {
			finished: std::sync::Arc<Mutex<bool>>,
		}
		#[async_trait]
		impl TileSink for FinishSink {
			async fn write_tile(&self, _: &TileCoord, _: &Blob) -> Result<()> {
				Ok(())
			}
			async fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				*self.finished.lock().unwrap() = true;
				Ok(())
			}
		}

		let sink = deduplicating_tile_sink(Box::new(FinishSink {
			finished: finished_clone,
		}));
		let runtime = TilesRuntime::new();
		sink.finish(&TileJSON::default(), &runtime).await?;
		assert!(*finished.lock().unwrap());
		Ok(())
	}

	// ─── open_tile_sink ───

	#[tokio::test]
	async fn test_open_tile_sink_tar() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.tar");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		// Should succeed and be writable
		sink.write_tile(&coord(0, 0, 0), &blob(b"data")).await?;
		Ok(())
	}

	#[tokio::test]
	async fn test_open_tile_sink_versatiles() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.versatiles");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		sink.write_tile(&coord(0, 0, 0), &blob(b"tile")).await?;
		Ok(())
	}

	#[tokio::test]
	async fn test_open_tile_sink_directory() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let out = dir.path().join("tiles");
		std::fs::create_dir(&out)?;
		let runtime = TilesRuntime::new();
		let _sink = open_tile_sink(
			out.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		Ok(())
	}

	#[tokio::test]
	async fn test_open_tile_sink_no_extension_creates_directory() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let out = dir.path().join("output_tiles");
		let runtime = TilesRuntime::new();
		let _sink = open_tile_sink(
			out.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		Ok(())
	}

	#[tokio::test]
	async fn test_open_tile_sink_unsupported_extension() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("out.xyz");
		let runtime = TilesRuntime::new();
		let result = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await;
		let err = result.err().expect("should fail for unsupported extension");
		assert!(err.to_string().contains("unsupported"));
	}

	#[tokio::test]
	async fn test_open_tile_sink_deduplicates() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.tar");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		// Writing the same coord twice should silently drop the second
		let c = coord(5, 3, 4);
		sink.write_tile(&c, &blob(b"first")).await?;
		sink.write_tile(&c, &blob(b"second")).await?; // should be dropped, no error
		Ok(())
	}

	#[tokio::test]
	async fn test_open_tile_sink_mbtiles() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.mbtiles");
		let runtime = TilesRuntime::new();
		let sink = open_tile_sink(
			path.to_str().unwrap(),
			TileFormat::PNG,
			TileCompression::Uncompressed,
			&runtime,
		)
		.await?;
		sink.write_tile(&coord(0, 0, 0), &blob(b"tile")).await?;
		Ok(())
	}
}
