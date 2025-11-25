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
//!     let registry = ContainerRegistry::default();
//!
//!     // Read from a local file
//!     let reader = registry.get_reader("../testdata/berlin.mbtiles").await.unwrap();
//!
//!     // Define the output filename
//!     let output_path = std::env::temp_dir().join("temp3.versatiles");
//!
//!     // Write the tiles to the output file
//!     registry.write_to_path(reader, &output_path).await.unwrap();
//!
//!     println!("Tiles have been successfully converted and saved to {output_path:?}");
//! }
//! ```

use crate::*;
use anyhow::{Result, bail};
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
use versatiles_core::io::{DataReader, DataReaderHttp};
#[cfg(test)]
use versatiles_core::{TileCompression, TileFormat};
use versatiles_derive::context;

/// Signature for async opener functions used by the registry.
type ReadFuture = Pin<Box<dyn Future<Output = Result<Box<dyn TilesReaderTrait>>> + Send>>;
type ReadData = Box<dyn Fn(DataReader) -> ReadFuture + Send + Sync + 'static>;
type ReadFile = Box<dyn Fn(PathBuf) -> ReadFuture + Send + Sync + 'static>;
type WriteFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
type WriteFile =
	Box<dyn Fn(Box<dyn TilesReaderTrait>, PathBuf, ProcessingConfig) -> WriteFuture + Send + Sync + 'static>;

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
	writer_config: ProcessingConfig,
}

impl ContainerRegistry {
	/// Creates a new `ContainerRegistry` with the specified writer configuration.
	///
	/// Registers built-in readers and writers for supported container formats.
	pub fn new(writer_config: ProcessingConfig) -> Self {
		let mut reg = Self {
			data_readers: HashMap::new(),
			file_readers: HashMap::new(),
			file_writers: HashMap::new(),
			writer_config,
		};

		// MBTiles
		reg.register_reader_file("mbtiles", |p| async move { Ok(MBTilesReader::open_path(&p)?.boxed()) });
		reg.register_writer_file("mbtiles", |mut r, p, c| async move {
			MBTilesWriter::write_to_path(r.as_mut(), &p, c).await
		});

		// TAR
		reg.register_reader_file("tar", |p| async move { Ok(TarTilesReader::open_path(&p)?.boxed()) });
		reg.register_writer_file("tar", |mut r, p, c| async move {
			TarTilesWriter::write_to_path(r.as_mut(), &p, c).await
		});
		// PMTiles
		reg.register_reader_file(
			"pmtiles",
			|p| async move { Ok(PMTilesReader::open_path(&p).await?.boxed()) },
		);
		reg.register_reader_data("pmtiles", |p| async move {
			Ok(PMTilesReader::open_reader(p).await?.boxed())
		});
		reg.register_writer_file("pmtiles", |mut r, p, c| async move {
			PMTilesWriter::write_to_path(r.as_mut(), &p, c).await
		});

		// VersaTiles
		reg.register_reader_file("versatiles", |p| async move {
			Ok(VersaTilesReader::open_path(&p).await?.boxed())
		});
		reg.register_reader_data("versatiles", |p| async move {
			Ok(VersaTilesReader::open_reader(p).await?.boxed())
		});
		reg.register_writer_file("versatiles", |mut r, p, c| async move {
			VersaTilesWriter::write_to_path(r.as_mut(), &p, c).await
		});

		reg
	}

	/// Register an async file-based reader for a given file extension.
	///
	/// # Arguments
	/// * `ext` - The file extension to associate with the reader.
	/// * `read_file` - Async function that takes a `PathBuf` and returns a boxed `TilesReaderTrait`.
	pub fn register_reader_file<F, Fut>(&mut self, ext: &str, read_file: F)
	where
		F: Fn(PathBuf) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<Box<dyn TilesReaderTrait>>> + Send + 'static,
	{
		self.file_readers.insert(
			sanitize_extension(ext),
			Arc::new(Box::new(move |p| Box::pin(read_file(p)))),
		);
	}

	/// Register an async data-based reader for a given file extension.
	///
	/// # Arguments
	/// * `ext` - The file extension to associate with the reader.
	/// * `read_data` - Async function that takes a `DataReader` and returns a boxed `TilesReaderTrait`.
	pub fn register_reader_data<F, Fut>(&mut self, ext: &str, read_data: F)
	where
		F: Fn(DataReader) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<Box<dyn TilesReaderTrait>>> + Send + 'static,
	{
		self.data_readers.insert(
			sanitize_extension(ext),
			Arc::new(Box::new(move |p| Box::pin(read_data(p)))),
		);
	}

	/// Register an async file-based writer for a given file extension.
	///
	/// # Arguments
	/// * `ext` - The file extension to associate with the writer.
	/// * `write_file` - Async function that takes a boxed `TilesReaderTrait`, a `PathBuf`, and a `ProcessingConfig`,
	///   and writes the tiles to the specified path.
	pub fn register_writer_file<F, Fut>(&mut self, ext: &str, write_file: F)
	where
		F: Fn(Box<dyn TilesReaderTrait>, PathBuf, ProcessingConfig) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<()>> + Send + 'static,
	{
		self.file_writers.insert(
			sanitize_extension(ext),
			Arc::new(Box::new(move |r, p, c| Box::pin(write_file(r, p, c)))),
		);
	}

	/// Get a tile container reader for a given filename or URL.
	///
	/// Resolves the path or URL, determines the file extension, and uses the appropriate registered reader.
	///
	/// # Arguments
	/// * `url_path` - The file path or URL to read from.
	///
	/// # Returns
	/// A boxed `TilesReaderTrait` for reading tiles.
	#[context("Failed to get reader for '{url_path:?}'")]
	pub async fn get_reader<T>(&self, url_path: T) -> Result<Box<dyn TilesReaderTrait>>
	where
		T: Into<UrlPath> + std::fmt::Debug + Clone,
	{
		let mut url_path = url_path.clone().into();
		url_path.resolve(&UrlPath::from(env::current_dir()?))?;

		let extension = sanitize_extension(&url_path.extension()?);

		match url_path {
			UrlPath::Url(url) => {
				let reader = DataReaderHttp::from_url(url.clone())
					.with_context(|| format!("Failed to create HTTP data reader for URL '{url}'"))?;

				let opener = self
					.data_readers
					.get(&extension)
					.ok_or_else(|| anyhow::anyhow!("file extension '{extension}' unknown"))?;
				opener(reader).await
			}
			UrlPath::Path(path) => {
				if !path.exists() {
					bail!("path '{path:?}' does not exist")
				}

				if path.is_dir() {
					return Ok(DirectoryTilesReader::open_path(&path)
						.with_context(|| format!("Failed opening {path:?} as directory"))?
						.boxed());
				}

				let opener = self
					.file_readers
					.get(&extension)
					.ok_or_else(|| anyhow::anyhow!("file extension '{extension}' unknown"))?;
				opener(path.to_path_buf()).await
			}
		}
	}

	/// Write tiles from a reader to the specified output path.
	///
	/// If the path is a directory, writes using the directory writer; otherwise, uses the appropriate file writer based on extension.
	///
	/// # Arguments
	/// * `reader` - A boxed tile container reader providing tiles to write.
	/// * `path` - The output path to write tiles to.
	///
	/// # Returns
	/// Result indicating success or failure.
	#[context("writing tiles to path '{path:?}'")]
	pub async fn write_to_path(&self, mut reader: Box<dyn TilesReaderTrait>, path: &Path) -> Result<()> {
		let path = env::current_dir()?.join(path);
		if path.is_dir() {
			return DirectoryTilesWriter::write_to_path(reader.as_mut(), &path, self.writer_config.clone()).await;
		}

		let extension = path
			.extension()
			.unwrap_or_default()
			.to_string_lossy()
			.to_ascii_lowercase();

		let writer = self
			.file_writers
			.get(&extension)
			.ok_or_else(|| anyhow::anyhow!("Error when reading: file extension '{extension}' unknown"))?;
		writer(reader, path.to_path_buf(), self.writer_config.clone()).await?;

		Ok(())
	}
}

fn sanitize_extension(ext: &str) -> String {
	ext.to_ascii_lowercase().trim_matches('.').to_string()
}

impl Default for ContainerRegistry {
	fn default() -> Self {
		Self::new(ProcessingConfig::default())
	}
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

	let registry = ContainerRegistry::new(ProcessingConfig::default());
	registry.write_to_path(Box::new(reader), container_file.path()).await?;

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

			let registry = ContainerRegistry::new(ProcessingConfig::default());
			registry.write_to_path(Box::new(reader1), &path).await?;

			// get test container reader using the default registry (back-compat)
			let mut reader2 = registry.get_reader(&UrlPath::from(path)).await?;
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
