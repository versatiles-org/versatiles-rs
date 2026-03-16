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
//!     let reader = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await.unwrap();
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

use crate::{
	DataSource, DirectoryReader, DirectoryWriter, MBTilesReader, MBTilesWriter, PMTilesReader, PMTilesWriter,
	SharedTileSource, TarTilesReader, TarTilesWriter, TileSource, TilesReader, TilesRuntime, TilesWriter,
	VersaTilesReader, VersaTilesWriter, types::data_location::DataLocation,
};
#[cfg(test)]
use crate::{MockReader, TileSourceMetadata, Traversal};
use anyhow::{Context, Result, anyhow, bail};
#[cfg(test)]
use assert_fs::NamedTempFile;
use std::{
	collections::HashMap,
	env,
	future::Future,
	path::{Path, PathBuf},
	pin::Pin,
	sync::Arc,
};
#[cfg(feature = "ssh2")]
use versatiles_core::io::DataWriterSftp;
use versatiles_core::io::{DataReader, DataReaderBlob, DataReaderHttp, DataWriterTrait};
#[cfg(test)]
use versatiles_core::{TileBBoxPyramid, TileCompression, TileFormat};
use versatiles_derive::context;

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
	write_to_writer: Option<Arc<WriteData>>,
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
	pub async fn get_reader_from_str(&self, data_source: &str, runtime: TilesRuntime) -> Result<SharedTileSource> {
		self.get_reader(DataSource::parse(data_source)?, runtime).await
	}

	/// Get a tile container reader for a given [`DataLocation`] (path or URL).
	pub async fn get_reader_from_location(
		&self,
		location: DataLocation,
		runtime: TilesRuntime,
	) -> Result<SharedTileSource> {
		let debug = format!("{location:?}");
		self
			.get_reader(DataSource::try_from(location)?, runtime)
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
	#[context("Failed to get reader for '{data_source:?}'")]
	pub async fn get_reader(&self, data_source: DataSource, runtime: TilesRuntime) -> Result<SharedTileSource> {
		let mut data_source = data_source.clone();
		data_source.resolve(&DataLocation::cwd()?)?;
		let extension = sanitize_extension(data_source.container_type()?);

		match data_source.into_location() {
			DataLocation::Url(url) => {
				let reader = DataReaderHttp::from_url(url.clone())
					.with_context(|| format!("Failed to create HTTP data reader for URL '{url}'"))?;

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
		(entry.write_to_path)(reader, path.clone(), runtime).await?;

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
		let remote_path = DataWriterSftp::path_from_url(url).ok_or_else(|| anyhow!("invalid SFTP URL: {url}"))?;

		let extension = sanitize_extension(&remote_path.extension().unwrap_or_default().to_string_lossy());

		let writer = DataWriterSftp::from_url(url)?;

		let entry = self
			.writers
			.get(&extension)
			.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?;
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
						W::write_to_writer(boxed.as_mut(), w.as_mut(), rt).await
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
	let reader = MockReader::new_mock(TileSourceMetadata::new(
		tile_format,
		compression,
		TileBBoxPyramid::new_full_up_to(max_zoom_level),
		Traversal::ANY,
	))?;

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
	use super::*;
	use crate::MockWriter;
	use assert_fs::TempDir;
	use std::time::Instant;
	use versatiles_core::TileBBoxPyramid;

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
			let reader1 = MockReader::new_mock(TileSourceMetadata::new(
				tile_format,
				compression,
				TileBBoxPyramid::new_full_up_to(2),
				Traversal::ANY,
			))?;

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
			let reader2 = registry.get_reader_from_str(path.to_str().unwrap(), runtime).await?;
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
