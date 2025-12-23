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

use crate::{TilesRuntime, types::data_location::DataLocation, *};
use anyhow::{Result, anyhow, bail};
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
use versatiles_core::io::{DataReader, DataReaderBlob, DataReaderHttp};
#[cfg(test)]
use versatiles_core::{TileCompression, TileFormat};
use versatiles_derive::context;

/// Signature for async opener functions used by the registry.
type ReadFuture = Pin<Box<dyn Future<Output = Result<Box<dyn TileSourceTrait>>> + Send>>;
type ReadData = Box<dyn Fn(DataReader, TilesRuntime) -> ReadFuture + Send + Sync + 'static>;
type ReadFile = Box<dyn Fn(PathBuf, TilesRuntime) -> ReadFuture + Send + Sync + 'static>;
type WriteFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
type WriteFile = Box<dyn Fn(Box<dyn TileSourceTrait>, PathBuf, TilesRuntime) -> WriteFuture + Send + Sync + 'static>;

/// Registry mapping file extensions to async tile container readers and writers.
///
/// Supports reading and writing of tile containers in formats such as:
/// - MBTiles
/// - TAR
/// - PMTiles
/// - VersaTiles
/// - Directory-based containers
#[derive(Clone)]
pub struct ContainerRegistry {
	data_readers: HashMap<String, Arc<ReadData>>,
	file_readers: HashMap<String, Arc<ReadFile>>,
	file_writers: HashMap<String, Arc<WriteFile>>,
}

impl ContainerRegistry {
	/// Creates a new `ContainerRegistry` with the specified runtime.
	///
	/// Registers built-in readers and writers for supported container formats.
	pub fn new_empty() -> Self {
		Self {
			data_readers: HashMap::new(),
			file_readers: HashMap::new(),
			file_writers: HashMap::new(),
		}
	}

	/// Register an async file-based reader for a given file extension.
	///
	/// # Arguments
	/// * `ext` - The file extension to associate with the reader.
	/// * `read_file` - Async function that takes a `PathBuf` and returns a boxed `TileSourceTrait`.
	pub fn register_reader_file<F, Fut>(&mut self, ext: &str, read_file: F)
	where
		F: Fn(PathBuf, TilesRuntime) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<Box<dyn TileSourceTrait>>> + Send + 'static,
	{
		self.file_readers.insert(
			sanitize_extension(ext),
			Arc::new(Box::new(move |p, r| Box::pin(read_file(p, r)))),
		);
	}

	/// Register an async data-based reader for a given file extension.
	///
	/// # Arguments
	/// * `ext` - The file extension to associate with the reader.
	/// * `read_data` - Async function that takes a `DataReader` and returns a boxed `TileSourceTrait`.
	pub fn register_reader_data<F, Fut>(&mut self, ext: &str, read_data: F)
	where
		F: Fn(DataReader, TilesRuntime) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<Box<dyn TileSourceTrait>>> + Send + 'static,
	{
		self.data_readers.insert(
			sanitize_extension(ext),
			Arc::new(Box::new(move |p, r| Box::pin(read_data(p, r)))),
		);
	}

	/// Register an async file-based writer for a given file extension.
	///
	/// # Arguments
	/// * `ext` - The file extension to associate with the writer.
	/// * `write_file` - Async function that takes a boxed `TileSourceTrait`, a `PathBuf`, and a `TilesRuntime`,
	///   and writes the tiles to the specified path.
	pub fn register_writer_file<F, Fut>(&mut self, ext: &str, write_file: F)
	where
		F: Fn(Box<dyn TileSourceTrait>, PathBuf, TilesRuntime) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<()>> + Send + 'static,
	{
		self.file_writers.insert(
			sanitize_extension(ext),
			Arc::new(Box::new(move |r, p, rt| Box::pin(write_file(r, p, rt)))),
		);
	}

	#[context("Failed to get reader from string '{data_source}'")]
	pub async fn get_reader_from_str(
		&self,
		data_source: &str,
		runtime: TilesRuntime,
	) -> Result<Box<dyn TileSourceTrait>> {
		self.get_reader(DataSource::parse(data_source)?, runtime).await
	}

	/// Get a tile container reader for a given filename or URL.
	///
	/// Resolves the path or URL, determines the file extension, and uses the appropriate registered reader.
	///
	/// # Arguments
	/// * `url_path` - The file path or URL to read from.
	///
	/// # Returns
	/// A boxed `TileSourceTrait` for reading tiles.
	#[context("Failed to get reader for '{data_source:?}'")]
	pub async fn get_reader(&self, data_source: DataSource, runtime: TilesRuntime) -> Result<Box<dyn TileSourceTrait>> {
		let mut data_source = data_source.clone();
		data_source.resolve(&DataLocation::cwd()?)?;
		let extension = sanitize_extension(data_source.container_type()?);

		match data_source.into_location() {
			DataLocation::Url(url) => {
				let reader = DataReaderHttp::from_url(url.clone())
					.with_context(|| format!("Failed to create HTTP data reader for URL '{url}'"))?;

				self
					.data_readers
					.get(&extension)
					.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?(reader, runtime)
				.await
			}
			DataLocation::Path(path) => {
				if !path.exists() {
					bail!("path '{path:?}' does not exist")
				}

				if path.is_dir() {
					return Ok(DirectoryTilesReader::open_path(&path)
						.with_context(|| format!("Failed opening {path:?} as directory"))?
						.boxed());
				}

				self
					.file_readers
					.get(&extension)
					.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?(path.to_path_buf(), runtime)
				.await
			}
			DataLocation::Blob(blob) => {
				let reader = Box::new(DataReaderBlob::from(blob));
				self
					.data_readers
					.get(&extension)
					.ok_or_else(|| anyhow!("file extension '{extension}' unknown"))?(reader, runtime)
				.await
			}
		}
	}

	/// Write tiles with a custom runtime for progress monitoring and other options.
	///
	/// # Arguments
	/// * `reader` - A boxed tile container reader providing tiles to write.
	/// * `path` - The output path to write tiles to.
	/// * `runtime` - Runtime configuration (cache type, event bus, progress factory, etc.).
	///
	/// # Returns
	/// Result indicating success or failure.
	#[context("writing tiles to path '{path:?}'")]
	pub async fn write_to_path(
		&self,
		mut reader: Box<dyn TileSourceTrait>,
		path: &Path,
		runtime: TilesRuntime,
	) -> Result<()> {
		let path = env::current_dir()?.join(path);

		if path.is_dir() {
			return DirectoryTilesWriter::write_to_path(reader.as_mut(), &path, runtime).await;
		}

		let extension = path
			.extension()
			.unwrap_or_default()
			.to_string_lossy()
			.to_ascii_lowercase();

		let writer = self
			.file_writers
			.get(&extension)
			.ok_or_else(|| anyhow!("Error when reading: file extension '{extension}' unknown"))?;
		writer(reader, path.to_path_buf(), runtime).await?;

		Ok(())
	}

	pub fn supports_reader_extension(&self, ext: &str) -> bool {
		let ext = sanitize_extension(ext);
		self.data_readers.contains_key(&ext) || self.file_readers.contains_key(&ext)
	}
}

impl Default for ContainerRegistry {
	fn default() -> Self {
		let mut reg = Self::new_empty();

		// MBTiles
		reg.register_reader_file(
			"mbtiles",
			|p, r| async move { Ok(MBTilesReader::open_path(&p, r)?.boxed()) },
		);
		reg.register_writer_file("mbtiles", |mut r, p, rt| async move {
			MBTilesWriter::write_to_path(r.as_mut(), &p, rt).await
		});

		// TAR
		reg.register_reader_file("tar", |p, _r| async move { Ok(TarTilesReader::open_path(&p)?.boxed()) });
		reg.register_writer_file("tar", |mut r, p, rt| async move {
			TarTilesWriter::write_to_path(r.as_mut(), &p, rt).await
		});
		// PMTiles
		reg.register_reader_file("pmtiles", |p, r| async move {
			Ok(PMTilesReader::open_path(&p, r).await?.boxed())
		});
		reg.register_reader_data("pmtiles", |p, r| async move {
			Ok(PMTilesReader::open_reader(p, r).await?.boxed())
		});
		reg.register_writer_file("pmtiles", |mut r, p, rt| async move {
			PMTilesWriter::write_to_path(r.as_mut(), &p, rt).await
		});

		// VersaTiles
		reg.register_reader_file("versatiles", |p, r| async move {
			Ok(VersaTilesReader::open_path(&p, r).await?.boxed())
		});
		reg.register_reader_data("versatiles", |p, r| async move {
			Ok(VersaTilesReader::open_reader(p, r).await?.boxed())
		});
		reg.register_writer_file("versatiles", |mut r, p, rt| async move {
			VersaTilesWriter::write_to_path(r.as_mut(), &p, rt).await
		});

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

	use versatiles_core::{TileBBoxPyramid, TilesReaderParameters};
	let reader = MockTilesReader::new_mock(TilesReaderParameters::new(
		tile_format,
		compression,
		TileBBoxPyramid::new_full(max_zoom_level),
	))?;

	// get to test container converter
	let container_file = match extension {
		"tar" => NamedTempFile::new("temp.tar"),
		"versatiles" => NamedTempFile::new("temp.versatiles"),
		_ => panic!("make_test_file: extension {extension} not found"),
	}?;

	let registry = ContainerRegistry::default();
	registry
		.write_to_path(Box::new(reader), container_file.path(), TilesRuntime::default())
		.await?;
	Ok(container_file)
}

#[cfg(test)]
/// Integration tests for container readers and writers across supported formats.
pub mod tests {
	use super::*;
	use assert_fs::TempDir;
	use std::time::Instant;
	use versatiles_core::{TileBBoxPyramid, TilesReaderParameters};

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
			let reader1 = MockTilesReader::new_mock(TilesReaderParameters::new(
				tile_format,
				compression,
				TileBBoxPyramid::new_full(2),
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
				.write_to_path(Box::new(reader1), &path, runtime.clone())
				.await?;

			// get test container reader using the default registry (back-compat)
			let mut reader2 = registry.get_reader_from_str(path.to_str().unwrap(), runtime).await?;
			MockTilesWriter::write(reader2.as_mut()).await?;

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
