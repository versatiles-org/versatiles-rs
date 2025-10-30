//! Module `getter` provides functionalities to read and write tile containers from various sources such as local files, directories, and URLs.
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
use anyhow::{Context, Result, bail};
#[cfg(test)]
use assert_fs::NamedTempFile;
use reqwest::Url;
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

/// Registry to map file extensions to async openers.
#[derive(Clone)]
pub struct ContainerRegistry {
	data_readers: HashMap<String, Arc<ReadData>>,
	file_readers: HashMap<String, Arc<ReadFile>>,
	file_writers: HashMap<String, Arc<WriteFile>>,
	writer_config: ProcessingConfig,
}

impl ContainerRegistry {
	/// Create an empty registry.
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

	pub fn register_reader_file<F, Fut>(&mut self, ext: &str, read_file: F)
	where
		F: Fn(PathBuf) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<Box<dyn TilesReaderTrait>>> + Send + 'static,
	{
		self
			.file_readers
			.insert(ext.to_string(), Arc::new(Box::new(move |p| Box::pin(read_file(p)))));
	}

	pub fn register_reader_data<F, Fut>(&mut self, ext: &str, read_data: F)
	where
		F: Fn(DataReader) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<Box<dyn TilesReaderTrait>>> + Send + 'static,
	{
		self
			.data_readers
			.insert(ext.to_string(), Arc::new(Box::new(move |p| Box::pin(read_data(p)))));
	}

	pub fn register_writer_file<F, Fut>(&mut self, ext: &str, write_file: F)
	where
		F: Fn(Box<dyn TilesReaderTrait>, PathBuf, ProcessingConfig) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<()>> + Send + 'static,
	{
		self.file_writers.insert(
			ext.to_string(),
			Arc::new(Box::new(move |r, p, c| Box::pin(write_file(r, p, c)))),
		);
	}

	/// Get a reader for a given filename or URL.
	#[context("Failed to get reader for '{filename}'")]
	pub async fn get_reader(&self, filename: &str) -> Result<Box<dyn TilesReaderTrait>> {
		let extension = get_extension(filename);

		// Try URL first
		if let Ok(reader) = parse_as_url(filename) {
			let opener = self
				.data_readers
				.get(&extension)
				.ok_or_else(|| anyhow::anyhow!("Error when reading: file extension '{extension}' unknown"))?;
			return opener(reader).await;
		}

		// Resolve local path
		let path = env::current_dir()?.join(filename);

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
			.ok_or_else(|| anyhow::anyhow!("Error when reading: file extension '{extension}' unknown"))?;
		opener(path).await
	}

	pub async fn write_to_filename(&self, reader: Box<dyn TilesReaderTrait>, filename: &str) -> Result<()> {
		let path = env::current_dir()?.join(filename);
		self.write_to_path(reader, &path).await
	}

	pub async fn write_to_path(&self, mut reader: Box<dyn TilesReaderTrait>, path: &Path) -> Result<()> {
		if path.is_dir() {
			return DirectoryTilesWriter::write_to_path(reader.as_mut(), path, self.writer_config.clone()).await;
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

impl Default for ContainerRegistry {
	fn default() -> Self {
		Self::new(ProcessingConfig::default())
	}
}

/// Parse a filename as a URL and return a DataReader if successful.
fn parse_as_url(filename: &str) -> Result<DataReader> {
	if filename.starts_with("http://") || filename.starts_with("https://") {
		Ok(DataReaderHttp::from_url(Url::parse(filename)?)?)
	} else {
		bail!("not an url")
	}
}

/// Get the file extension from a filename.
fn get_extension(filename: &str) -> String {
	filename
		.split('?')
		.next()
		.map(|filename| filename.rsplit('.').next().unwrap_or(""))
		.unwrap_or("")
		.to_ascii_lowercase()
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
	registry
		.write_to_filename(Box::new(reader), container_file.to_str().unwrap())
		.await?;

	Ok(container_file)
}

#[cfg(test)]
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

			enum TempType {
				Dir(TempDir),
				File(NamedTempFile),
			}

			// get to test container converter
			let path: TempType = match container {
				Container::Directory => TempType::Dir(TempDir::new()?),
				Container::Tar => TempType::File(NamedTempFile::new("temp.tar")?),
				Container::Versatiles => TempType::File(NamedTempFile::new("temp.versatiles")?),
			};

			let filename = match &path {
				TempType::Dir(t) => t.to_str().unwrap(),
				TempType::File(t) => t.to_str().unwrap(),
			};

			let registry = ContainerRegistry::new(ProcessingConfig::default());
			registry.write_to_filename(Box::new(reader1), filename).await?;

			// get test container reader using the default registry (back-compat)
			let mut reader2 = registry.get_reader(filename).await?;
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
