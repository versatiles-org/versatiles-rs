//! `ContainerRegistry` provides functionalities to read and write tile containers from various sources such as local files, directories, and URLs.
//!
//! It supports multiple container formats and allows registering custom readers and writers for different file extensions.
//!
//! # Example Usage
//!
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Use the default registry
//!     let runtime = TilesRuntime::default();
//!
//!     // Read from a local file
//!     let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await.unwrap();
//!
//!     // Define the output filename
//!     let output_path = std::env::temp_dir().join("temp3.versatiles");
//!
//!     // Write the tiles to the output file
//!     runtime.write_to_path(reader, &output_path).await.unwrap();
//!
//!     println!("Tiles have been successfully converted and saved to {output_path:?}");
//! }
//! ```

use std::{
	collections::{BTreeMap, HashMap},
	env,
	future::Future,
	path::{Path, PathBuf},
	pin::Pin,
	sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(test)]
use assert_fs::NamedTempFile;
use versatiles_core::io::{DataReader, DataReaderBlob, DataReaderHttp, DataWriterTrait};
#[cfg(feature = "ssh2")]
use versatiles_core::io::{DataReaderSftp, DataWriterSftp};
#[cfg(test)]
use versatiles_core::{TileCompression, TileFormat, TilePyramid};
use versatiles_derive::context;

use crate::{
	DataSource, DirectoryReader, DirectoryWriter, MBTilesReader, MBTilesWriter, PMTilesReader, PMTilesWriter,
	SharedTileSource, TarTilesReader, TarTilesWriter, TileSource, TilesReader, TilesRuntime, TilesWriter,
	VersaTilesReader, VersaTilesWriter, types::data_location::DataLocation,
};
#[cfg(test)]
use crate::{MockReader, TileSourceMetadata, Traversal};

/// Signature for async opener functions used by the registry.
type ReadFuture = Pin<Box<dyn Future<Output = Result<SharedTileSource>> + Send>>;
type ReadData = Box<dyn Fn(DataReader, TilesRuntime) -> ReadFuture + Send + Sync + 'static>;
type ReadFile = Box<dyn Fn(PathBuf, TilesRuntime) -> ReadFuture + Send + Sync + 'static>;
type WriteFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
type WriteFile = Box<dyn Fn(SharedTileSource, PathBuf, TilesRuntime) -> WriteFuture + Send + Sync + 'static>;
type WriteData =
	Box<dyn Fn(SharedTileSource, Box<dyn DataWriterTrait>, TilesRuntime) -> WriteFuture + Send + Sync + 'static>;

#[derive(Clone)]
struct ReaderEntry {
	open_path: Arc<ReadFile>,
	open_reader: Option<Arc<ReadData>>,
}

#[derive(Clone)]
struct WriterEntry {
	write_to_path: Arc<WriteFile>,
	#[cfg_attr(not(feature = "ssh2"), allow(dead_code))]
	write_to_writer: Option<Arc<WriteData>>,
	/// Option keys this writer reads, from `TilesWriter::supported_options`.
	supported_options: &'static [&'static str],
}

/// Registry mapping file extensions to async tile container readers and writers.
///
/// Supports reading and writing of tile containers in formats such as:
/// - `MBTiles`
/// - TAR
/// - `PMTiles`
/// - `VersaTiles`
/// - Directory-based containers
#[derive(Clone)]
pub struct ContainerRegistry {
	readers: HashMap<String, ReaderEntry>,
	writers: HashMap<String, WriterEntry>,
}

/// Rejects writer options the chosen writer will never read.
///
/// Runs before anything is opened, so a mistyped key fails immediately instead
/// of being silently ignored — the same reason the PMTiles traversal check runs
/// before the first tile is read.
fn check_writer_options(options: &BTreeMap<String, String>, entry: &WriterEntry, extension: &str) -> Result<()> {
	let unknown: Vec<&str> = options
		.keys()
		.map(String::as_str)
		.filter(|key| !entry.supported_options.contains(key))
		.collect();

	if unknown.is_empty() {
		return Ok(());
	}

	let known = if entry.supported_options.is_empty() {
		"it accepts none".to_string()
	} else {
		format!("it accepts: {}", entry.supported_options.join(", "))
	};
	bail!(
		"unknown writer option{} for '{extension}': {} — {known}",
		if unknown.len() == 1 { "" } else { "s" },
		unknown.join(", ")
	);
}

/// Removes an output file a failed write left behind.
///
/// A writer opens its destination before it knows whether the conversion will
/// succeed — `DataWriterFile::from_path` calls `File::create`, and `MBTilesWriter`
/// creates its database — so a failure at any later point leaves a file that looks
/// like a real output. Even the earliest possible failure leaves a zero-byte one.
///
/// **Only ever called for a path that did not exist before the write started.** An
/// output that overwrites an existing file is left alone: the writer may have
/// failed before opening it, and deleting a file the caller already had would turn
/// a failed conversion into data loss. The truncated-overwrite case is not
/// recoverable either way, and losing the user's file is the worse of the two.
///
/// Best-effort: a failure to remove is logged, never propagated, because it must
/// not replace the real error that caused the write to fail.
fn remove_partial_output(path: &Path) {
	if !path.is_file() {
		return;
	}
	match std::fs::remove_file(path) {
		Ok(()) => log::debug!("removed partial output {path:?} after a failed write"),
		Err(e) => log::warn!("could not remove partial output {path:?} after a failed write: {e}"),
	}
}

impl ContainerRegistry {
	/// Creates a new `ContainerRegistry` with the specified runtime.
	///
	/// Registers built-in readers and writers for supported container formats.
	#[must_use]
	pub fn new_empty() -> Self {
		Self {
			readers: HashMap::new(),
			writers: HashMap::new(),
		}
	}

	#[context("Failed to get reader from string '{data_source}'")]
	pub async fn reader_from_str(&self, data_source: &str, runtime: TilesRuntime) -> Result<SharedTileSource> {
		self.reader(DataSource::parse(data_source)?, runtime).await
	}

	/// Get a tile container reader for a given [`DataLocation`] (path or URL).
	pub async fn reader_from_location(&self, location: DataLocation, runtime: TilesRuntime) -> Result<SharedTileSource> {
		self
			.reader_from_location_with_ssh_identity(location, runtime, None)
			.await
	}

	/// Get a tile container reader, authenticating SFTP with `ssh_identity`.
	///
	/// The identity applies to this source alone and takes precedence over the
	/// runtime's, which is process-wide. That is what lets one pipeline read from
	/// two SFTP hosts that need different keys.
	pub async fn reader_from_location_with_ssh_identity(
		&self,
		location: DataLocation,
		runtime: TilesRuntime,
		ssh_identity: Option<&Path>,
	) -> Result<SharedTileSource> {
		let debug = format!("{location:?}");
		self
			.reader_with_ssh_identity(DataSource::try_from(location)?, runtime, ssh_identity)
			.await
			.with_context(|| format!("Failed to get reader from location '{debug}'"))
	}

	/// Get a tile container reader for a given filename or URL.
	///
	/// Resolves the path or URL, determines the file extension, and uses the appropriate registered reader.
	///
	/// # Arguments
	/// * `url_path` - The file path or URL to read from.
	///
	/// # Returns
	/// A `SharedTileSource` for reading tiles.
	pub async fn reader(&self, data_source: DataSource, runtime: TilesRuntime) -> Result<SharedTileSource> {
		self.reader_with_ssh_identity(data_source, runtime, None).await
	}

	/// As [`Self::reader`], with an SFTP identity for this source alone.
	#[context("Failed to get reader for '{data_source:?}'")]
	pub async fn reader_with_ssh_identity(
		&self,
		data_source: DataSource,
		runtime: TilesRuntime,
		ssh_identity: Option<&Path>,
	) -> Result<SharedTileSource> {
		let mut data_source = data_source.clone();
		data_source.resolve(&DataLocation::cwd()?)?;
		let extension = sanitize_extension(data_source.container_type()?);

		let started = std::time::Instant::now();
		let label = data_source.location().to_string();
		log::trace!("registry: opening '{label}' (.{extension})");
		let result = self.reader_impl(data_source, runtime, extension, ssh_identity).await;
		log::trace!(
			"registry: '{label}' → {} in {:.2}s",
			if result.is_ok() { "ok" } else { "err" },
			started.elapsed().as_secs_f32()
		);
		result
	}

	// `ssh_identity` is read only by the SFTP branch, which is compiled out
	// without the `ssh2` feature.
	#[cfg_attr(not(feature = "ssh2"), allow(unused_variables))]
	async fn reader_impl(
		&self,
		data_source: DataSource,
		runtime: TilesRuntime,
		extension: String,
		ssh_identity: Option<&Path>,
	) -> Result<SharedTileSource> {
		match data_source.into_location() {
			DataLocation::Url(url) => {
				let reader: DataReader = match url.scheme() {
					#[cfg(feature = "ssh2")]
					"sftp" => {
						// SFTP open does a blocking SSH handshake. Off-load it to
						// the tokio blocking pool so concurrent opens (e.g. from
						// `from_stacked_raster` building several sub-pipelines at
						// once) actually run in parallel instead of serializing
						// on the async task's worker thread.
						// A key named on the source itself wins over the process-wide one.
						let identity = ssh_identity
							.or_else(|| runtime.ssh_identity())
							.map(std::path::Path::to_path_buf);
						let url_for_open = url.clone();
						let reader =
							tokio::task::spawn_blocking(move || DataReaderSftp::open(&url_for_open, identity.as_deref()))
								.await
								.with_context(|| format!("SFTP open task panicked for '{url}'"))?
								.with_context(|| format!("Failed to create SFTP data reader for URL '{url}'"))?;
						Box::new(reader)
					}
					"http" | "https" => Box::new(
						DataReaderHttp::try_from(&url)
							.with_context(|| format!("Failed to create HTTP data reader for URL '{url}'"))?,
					),
					scheme => bail!("unsupported URL scheme '{scheme}' in '{url}'"),
				};

				let entry = self
					.readers
					.get(&extension)
					.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?;
				let open_reader = entry
					.open_reader
					.as_ref()
					.ok_or_else(|| anyhow!("file extension '{extension}' does not support URL reading"))?;
				open_reader(reader, runtime).await
			}
			DataLocation::Path(path) => {
				if !path.exists() {
					bail!("path '{path:?}' does not exist")
				}

				if path.is_dir() {
					return Ok(DirectoryReader::open(&path)
						.with_context(|| format!("Failed opening {path:?} as directory"))?
						.into_shared());
				}

				let entry = self
					.readers
					.get(&extension)
					.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?;
				(entry.open_path)(path.clone(), runtime).await
			}
			DataLocation::Blob(blob) => {
				let reader = Box::new(DataReaderBlob::from(blob));
				let entry = self
					.readers
					.get(&extension)
					.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?;
				let open_reader = entry
					.open_reader
					.as_ref()
					.ok_or_else(|| anyhow!("file extension '{extension}' does not support blob reading"))?;
				open_reader(reader, runtime).await
			}
		}
	}

	/// Write tiles with a custom runtime for progress monitoring and other options.
	///
	/// # Arguments
	/// * `reader` - A shared tile source providing tiles to write.
	/// * `path` - The output path to write tiles to.
	/// * `runtime` - Runtime configuration (cache type, event bus, progress factory, etc.).
	///
	/// # Returns
	/// Result indicating success or failure.
	#[context("writing tiles to path '{path:?}'")]
	pub async fn write_to_path(&self, reader: SharedTileSource, path: &Path, runtime: TilesRuntime) -> Result<()> {
		let path = env::current_dir()?.join(path);

		if path.is_dir() {
			let mut boxed_reader = Arc::try_unwrap(reader)
				.map_err(|_| anyhow!("Cannot get exclusive access to reader for directory write"))?;
			return DirectoryWriter::write_to_path(boxed_reader.as_mut(), &path, runtime).await;
		}

		let extension = sanitize_extension(&path.extension().unwrap_or_default().to_string_lossy());

		let entry = self
			.writers
			.get(&extension)
			.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?;

		check_writer_options(&runtime.writer_options(), entry, &extension)?;

		// Whether the destination already existed decides whether a failed write
		// may clean up after itself — see `remove_partial_output`.
		let existed_before = path.exists();
		let result = (entry.write_to_path)(reader, path.clone(), runtime).await;
		if result.is_err() && !existed_before {
			remove_partial_output(&path);
		}
		result?;

		Ok(())
	}

	/// Write tiles to a destination specified as a string (path or SFTP URL).
	///
	/// Detects `sftp://` URLs and writes via SFTP when the `ssh2` feature is enabled.
	/// Otherwise falls back to path-based writing.
	pub async fn write_to_str(&self, reader: SharedTileSource, destination: &str, runtime: TilesRuntime) -> Result<()> {
		#[cfg(feature = "ssh2")]
		if destination.starts_with("sftp://") {
			return self.write_to_sftp(reader, destination, runtime).await;
		}

		let path = Path::new(destination);
		self.write_to_path(reader, path, runtime).await
	}

	/// Write tiles to a remote SFTP destination.
	#[cfg(feature = "ssh2")]
	#[context("writing tiles to SFTP '{url}'")]
	async fn write_to_sftp(&self, reader: SharedTileSource, url: &str, runtime: TilesRuntime) -> Result<()> {
		let url = reqwest::Url::parse(url).with_context(|| format!("invalid SFTP URL: {url}"))?;
		let remote_path = DataWriterSftp::path_from_url(&url);

		let extension = sanitize_extension(&remote_path.extension().unwrap_or_default().to_string_lossy());

		let writer = DataWriterSftp::from_url(&url, runtime.ssh_identity())?;

		let entry = self
			.writers
			.get(&extension)
			.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?;
		check_writer_options(&runtime.writer_options(), entry, &extension)?;

		let write_to_writer = entry.write_to_writer.as_ref().ok_or_else(|| {
			anyhow!(
				"file extension '{extension}' does not support writing to SFTP \
					 (only formats with data writers are supported, e.g. versatiles, pmtiles)"
			)
		})?;
		write_to_writer(reader, Box::new(writer), runtime).await
	}

	/// Register both file and (optionally) data readers for a [`TilesReader`] implementation.
	///
	/// The file reader is always registered. The data reader is only registered when
	/// `R::supports_data_reader()` returns `true`.
	pub fn register_reader<R: TilesReader + 'static>(&mut self, ext: &str) {
		let open_reader = if R::supports_data_reader() {
			Some(Arc::new(
				Box::new(|r, rt| Box::pin(R::open_reader(r, rt)) as ReadFuture) as ReadData,
			))
		} else {
			None
		};
		self.readers.insert(
			sanitize_extension(ext),
			ReaderEntry {
				open_path: Arc::new(Box::new(|p, rt| Box::pin(async move { R::open_path(&p, rt).await }))),
				open_reader,
			},
		);
	}

	/// Register both file and (optionally) data writers for a [`TilesWriter`] implementation.
	///
	/// The file writer is always registered. The data writer is only registered when
	/// `W::supports_data_writer()` returns `true`.
	pub fn register_writer<W: TilesWriter + 'static>(&mut self, ext: &str) {
		let write_to_writer = if W::supports_data_writer() {
			Some(Arc::new(
				Box::new(|r: SharedTileSource, mut w: Box<dyn DataWriterTrait>, rt| {
					Box::pin(async move {
						let mut boxed = Arc::try_unwrap(r).map_err(|_| anyhow!("Cannot get exclusive access to reader"))?;
						W::write_to_writer(boxed.as_mut(), w.as_mut(), rt).await?;
						// Flush any buffered data (e.g. the SFTP writer's coalesced blocks).
						w.finalize()
					}) as WriteFuture
				}) as WriteData,
			))
		} else {
			None
		};
		self.writers.insert(
			sanitize_extension(ext),
			WriterEntry {
				write_to_path: Arc::new(Box::new(|r, p, rt| {
					Box::pin(async move {
						let mut boxed = Arc::try_unwrap(r).map_err(|_| anyhow!("Cannot get exclusive access to reader"))?;
						W::write_to_path(boxed.as_mut(), &p, rt).await
					})
				})),
				write_to_writer,
				supported_options: W::supported_options(),
			},
		);
	}

	#[must_use]
	pub fn supports_reader_extension(&self, ext: &str) -> bool {
		let ext = sanitize_extension(ext);
		self.readers.contains_key(&ext)
	}

	#[must_use]
	pub fn supports_writer_extension(&self, ext: &str) -> bool {
		let ext = sanitize_extension(ext);
		self.writers.contains_key(&ext)
	}
}

impl Default for ContainerRegistry {
	fn default() -> Self {
		let mut reg = Self::new_empty();

		reg.register_reader::<MBTilesReader>("mbtiles");
		reg.register_writer::<MBTilesWriter>("mbtiles");

		reg.register_reader::<TarTilesReader>("tar");
		reg.register_writer::<TarTilesWriter>("tar");

		reg.register_reader::<PMTilesReader>("pmtiles");
		reg.register_writer::<PMTilesWriter>("pmtiles");

		reg.register_reader::<VersaTilesReader>("versatiles");
		reg.register_writer::<VersaTilesWriter>("versatiles");

		reg
	}
}

fn sanitize_extension(ext: &str) -> String {
	ext.to_ascii_lowercase().trim_matches('.').to_string()
}

#[cfg(test)]
/// Create a test file with given parameters.
pub async fn make_test_file(
	tile_format: TileFormat,
	compression: TileCompression,
	max_zoom_level: u8,
	extension: &str,
) -> Result<NamedTempFile> {
	// get dummy reader
	let reader = MockReader::new_mock(
		TilePyramid::new_full_up_to(max_zoom_level),
		TileSourceMetadata::new(tile_format, compression, Traversal::ANY, None),
	)?;

	// get to test container converter
	let container_file = match extension {
		"tar" => NamedTempFile::new("temp.tar"),
		"versatiles" => NamedTempFile::new("temp.versatiles"),
		_ => panic!("make_test_file: extension {extension} not found"),
	}?;

	let registry = ContainerRegistry::default();
	registry
		.write_to_path(
			Arc::new(Box::new(reader)),
			container_file.path(),
			TilesRuntime::default(),
		)
		.await?;
	Ok(container_file)
}

#[cfg(test)]
/// Integration tests for container readers and writers across supported formats.
pub mod tests {
	use std::time::Instant;

	use assert_fs::TempDir;

	use super::*;
	use crate::MockWriter;

	#[test]
	fn sanitize_extension_handles_dots_and_case() {
		assert_eq!(sanitize_extension("TAR"), "tar");
		assert_eq!(sanitize_extension(".tar"), "tar");
		assert_eq!(sanitize_extension(".TAR"), "tar");
		assert_eq!(sanitize_extension("...Versatiles..."), "versatiles");
		assert_eq!(sanitize_extension(""), "");
	}

	#[test]
	fn supports_reader_extension_known_and_unknown() {
		let reg = ContainerRegistry::default();
		assert!(reg.supports_reader_extension("tar"));
		assert!(reg.supports_reader_extension("TAR"));
		assert!(reg.supports_reader_extension(".tar"));
		assert!(reg.supports_reader_extension("versatiles"));
		assert!(!reg.supports_reader_extension("unknown"));
		assert!(!reg.supports_reader_extension(""));
	}

	/// A source whose traversal order PMTiles cannot accept, so `write_to_path`
	/// fails after the destination file has already been opened.
	fn source_pmtiles_rejects() -> Result<SharedTileSource> {
		let reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(2),
			TileSourceMetadata::new(
				TileFormat::PNG,
				TileCompression::Uncompressed,
				Traversal::new(crate::traversal::TraversalOrder::DepthFirst, 1, 16)?,
				None,
			),
		)?;
		Ok(Arc::new(Box::new(reader)))
	}

	#[tokio::test]
	async fn failed_write_removes_the_file_it_created() -> Result<()> {
		let dir = TempDir::new()?;
		let path = dir.path().join("out.pmtiles");
		let registry = ContainerRegistry::default();

		let result = registry
			.write_to_path(source_pmtiles_rejects()?, &path, TilesRuntime::new_silent())
			.await;

		assert!(result.is_err(), "the write was expected to fail");
		assert!(
			!path.exists(),
			"a failed write must not leave a file that looks like an output"
		);
		Ok(())
	}

	#[tokio::test]
	async fn failed_write_keeps_a_file_it_did_not_create() -> Result<()> {
		// Deleting a destination the caller already had would turn a failed
		// conversion into data loss, so an overwrite is left alone.
		let dir = TempDir::new()?;
		let path = dir.path().join("out.pmtiles");
		std::fs::write(&path, b"pre-existing")?;
		let registry = ContainerRegistry::default();

		let result = registry
			.write_to_path(source_pmtiles_rejects()?, &path, TilesRuntime::new_silent())
			.await;

		assert!(result.is_err(), "the write was expected to fail");
		assert!(path.exists(), "a pre-existing destination must not be removed");
		Ok(())
	}

	#[tokio::test]
	async fn successful_write_keeps_its_output() -> Result<()> {
		let dir = TempDir::new()?;
		let path = dir.path().join("out.versatiles");
		let reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(2),
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;
		let registry = ContainerRegistry::default();

		registry
			.write_to_path(Arc::new(Box::new(reader)), &path, TilesRuntime::new_silent())
			.await?;

		assert!(path.is_file(), "a successful write must keep its output");
		assert!(std::fs::metadata(&path)?.len() > 0);
		Ok(())
	}

	#[test]
	fn remove_partial_output_is_a_no_op_for_a_missing_or_non_file_path() {
		let dir = TempDir::new().unwrap();
		// Neither of these must panic, and the directory must survive: cleanup
		// never removes a directory output.
		remove_partial_output(&dir.path().join("does-not-exist"));
		remove_partial_output(dir.path());
		assert!(dir.path().is_dir(), "cleanup must never remove a directory");
	}

	#[tokio::test]
	async fn unknown_writer_option_is_rejected_before_anything_is_opened() -> Result<()> {
		let dir = TempDir::new()?;
		let path = dir.path().join("out.versatiles");
		let reader = MockReader::new_mock(
			TilePyramid::new_full_up_to(1),
			TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
		)?;
		let runtime = TilesRuntime::builder()
			.writer_option("allow_unclusterd", "true")
			.build();

		let err = ContainerRegistry::default()
			.write_to_path(Arc::new(Box::new(reader)), &path, runtime)
			.await
			.unwrap_err();
		// #[context] wraps the cause, so compare against the full chain.
		let err = format!("{err:#}");

		assert!(err.contains("unknown writer option"), "unexpected: {err}");
		assert!(err.contains("allow_unclusterd"), "should quote the key: {err}");
		assert!(err.contains("it accepts none"), "should say what is accepted: {err}");
		assert!(
			!path.exists(),
			"a rejected option must fail before the destination is opened"
		);
		Ok(())
	}

	#[test]
	fn unknown_option_message_lists_what_the_writer_accepts() {
		let mut reg = ContainerRegistry::new_empty();
		reg.register_writer::<MockWriter>("mock");
		let entry = reg.writers.get("mock").unwrap();

		let options = BTreeMap::from([("nope".to_string(), "1".to_string())]);
		let err = check_writer_options(&options, entry, "mock").unwrap_err().to_string();
		assert!(err.contains("nope"), "unexpected: {err}");
		assert!(err.contains("mock_option, mock_flag"), "should list the keys: {err}");

		// Plural wording when several are wrong.
		let options = BTreeMap::from([("a".to_string(), "1".to_string()), ("b".to_string(), "2".to_string())]);
		let err = check_writer_options(&options, entry, "mock").unwrap_err().to_string();
		assert!(err.contains("unknown writer options"), "should pluralise: {err}");
	}

	#[test]
	fn supported_options_are_accepted() {
		let mut reg = ContainerRegistry::new_empty();
		reg.register_writer::<MockWriter>("mock");
		let entry = reg.writers.get("mock").unwrap();

		let options = BTreeMap::from([
			("mock_option".to_string(), "x".to_string()),
			("mock_flag".to_string(), "true".to_string()),
		]);
		assert!(check_writer_options(&options, entry, "mock").is_ok());
		assert!(check_writer_options(&BTreeMap::new(), entry, "mock").is_ok());
	}

	#[test]
	fn supports_writer_extension_known_and_unknown() {
		let reg = ContainerRegistry::default();
		assert!(reg.supports_writer_extension("tar"));
		assert!(reg.supports_writer_extension("versatiles"));
		assert!(!reg.supports_writer_extension("unknown"));
		assert!(!reg.supports_writer_extension(""));
	}

	#[test]
	fn empty_registry_rejects_all_extensions() {
		let reg = ContainerRegistry::new_empty();
		assert!(!reg.supports_reader_extension("tar"));
		assert!(!reg.supports_reader_extension("versatiles"));
		assert!(!reg.supports_writer_extension("tar"));
		assert!(!reg.supports_writer_extension("versatiles"));
	}

	#[test]
	fn unknown_extension_read_error() {
		#[tokio::main]
		async fn inner() -> Result<()> {
			let reg = ContainerRegistry::default();
			let runtime = TilesRuntime::default();
			let err = reg.reader_from_str("file.unknown_ext", runtime).await.unwrap_err();
			assert!(err.to_string().contains("unknown"), "unexpected error: {err}");
			Ok(())
		}
		inner().unwrap();
	}

	#[test]
	fn unknown_extension_write_error() {
		#[tokio::main]
		async fn inner() -> Result<()> {
			let reg = ContainerRegistry::default();
			let runtime = TilesRuntime::default();
			let reader = MockReader::new_mock(
				TilePyramid::new_full_up_to(0),
				TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
			)?;
			let path = std::env::temp_dir().join("file.unknown_ext");
			let err = reg
				.write_to_path(Arc::new(Box::new(reader)), &path, runtime)
				.await
				.unwrap_err();
			assert!(err.to_string().contains("unknown"), "unexpected error: {err}");
			Ok(())
		}
		inner().unwrap();
	}

	#[test]
	fn register_same_extension_overwrites() {
		let mut reg = ContainerRegistry::new_empty();
		reg.register_reader::<VersaTilesReader>("custom");
		reg.register_reader::<TarTilesReader>("custom");
		// Should still work — the second registration replaces the first
		assert!(reg.supports_reader_extension("custom"));
	}

	#[test]
	fn blob_reading() {
		#[tokio::main]
		async fn inner() -> Result<()> {
			// Create a versatiles file via temp file, then read it back as a blob
			let reader = MockReader::new_mock(
				TilePyramid::new_full_up_to(1),
				TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None),
			)?;

			let temp = assert_fs::NamedTempFile::new("blob_test.versatiles")?;
			let reg = ContainerRegistry::default();
			let runtime = TilesRuntime::default();
			reg.write_to_path(Arc::new(Box::new(reader)), temp.path(), runtime.clone())
				.await?;

			// Read the file into a blob and open it via the data reader path
			let blob: Vec<u8> = std::fs::read(temp.path())?;
			let data_reader: DataReader = Box::new(DataReaderBlob::from(blob));
			let entry = reg.readers.get("versatiles").expect("versatiles reader must exist");
			let open_reader = entry
				.open_reader
				.as_ref()
				.expect("versatiles must support data reading");
			let _tile_source = open_reader(data_reader, runtime).await?;

			Ok(())
		}
		inner().unwrap();
	}

	/// Test writers and readers for various formats.
	#[test]
	fn writers_and_readers() -> Result<()> {
		#[derive(Debug)]
		enum Container {
			Directory,
			Tar,
			Versatiles,
		}

		#[tokio::main]
		async fn test_writer_and_reader(
			container: &Container,
			tile_format: TileFormat,
			compression: TileCompression,
		) -> Result<()> {
			let _test_name = format!("{container:?}, {tile_format:?}, {compression:?}");

			let _start = Instant::now();

			// get dummy reader
			let reader1 = MockReader::new_mock(
				TilePyramid::new_full_up_to(2),
				TileSourceMetadata::new(tile_format, compression, Traversal::ANY, None),
			)?;

			let path = TempDir::new()?.to_path_buf();
			if !path.exists() {
				std::fs::create_dir(&path)?;
			}

			// get to test container converter
			let path: PathBuf = match container {
				Container::Directory => path,
				Container::Tar => path.join("temp.tar"),
				Container::Versatiles => path.join("temp.versatiles"),
			};

			let registry = ContainerRegistry::default();
			let runtime = TilesRuntime::default();
			registry
				.write_to_path(Arc::new(Box::new(reader1)), &path, runtime.clone())
				.await?;

			// get test container reader using the default registry (back-compat)
			let reader2 = registry.reader_from_str(path.to_str().unwrap(), runtime).await?;
			let mut boxed =
				Arc::try_unwrap(reader2).map_err(|_| anyhow!("Cannot get exclusive access to reader for test"))?;
			MockWriter::write(boxed.as_mut()).await?;

			Ok(())
		}

		let containers = vec![Container::Directory, Container::Tar, Container::Versatiles];

		for container in containers {
			test_writer_and_reader(&container, TileFormat::PNG, TileCompression::Uncompressed)?;
			test_writer_and_reader(&container, TileFormat::JPG, TileCompression::Uncompressed)?;
			test_writer_and_reader(&container, TileFormat::MVT, TileCompression::Gzip)?;
		}

		Ok(())
	}
}
