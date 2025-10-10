//! This module provides functionality for reading data from files.
//!
//! # Overview
//!
//! The `DataReaderFile` struct allows for reading data stored in files. It implements the
//! `DataReaderTrait` to provide asynchronous reading capabilities and the standard library's
//! `Read` trait for synchronous reading. The module ensures the file exists, is absolute,
//! and is a regular file before attempting to open it.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::{DataReaderFile, DataReaderTrait}, Blob, ByteRange};
//! use anyhow::Result;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!      let path = std::env::current_dir()?.parent().unwrap().join("LICENSE");
//!      let mut reader = DataReaderFile::open(&path)?;
//!
//!      // Reading all data
//!      let all_data = reader.read_range(&ByteRange::new(4,7)).await?;
//!      assert_eq!(all_data.as_slice(), b"License");
//!
//!     Ok(())
//! }
//! ```

use super::DataReaderTrait;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use std::{
	fs::File,
	io::{Read, Seek, SeekFrom},
	path::Path,
};

/// A struct that provides reading capabilities from a file.
#[derive(Debug)]
pub struct DataReaderFile {
	name: String,
	file: File,
	size: u64,
}

impl DataReaderFile {
	/// Opens a file and creates a `DataReaderFile` instance.
	///
	/// # Arguments
	///
	/// * `path` - A reference to the file path to open.
	///
	/// # Returns
	///
	/// * A Result containing a boxed `DataReaderFile` or an error.
	pub fn open(path: &Path) -> Result<Box<DataReaderFile>> {
		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.is_file(), "path {path:?} must be a file");

		let path = path.canonicalize()?;
		let file = File::open(&path)?;
		let size = file.metadata()?.len();

		Ok(Box::new(DataReaderFile {
			name: path.to_str().unwrap().to_owned(),
			file,
			size,
		}))
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderFile {
	/// Reads a specific range of bytes from the file.
	///
	/// # Arguments
	///
	/// * `range` - A `ByteRange` struct specifying the offset and length of the range to read.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with the read data or an error.
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let mut buffer = vec![0; range.length as usize];
		let mut file = self
			.file
			.try_clone()
			.with_context(|| format!("failed to clone file '{}'", self.name))?;
		file
			.seek(SeekFrom::Start(range.offset))
			.with_context(|| format!("failed to seek to offset {} in file '{}',", range.offset, self.name))?;
		file.read_exact(&mut buffer).with_context(|| {
			format!(
				"failed to read {} bytes at offset {} in file '{}'",
				range.length, range.offset, self.name
			)
		})?;
		Ok(Blob::from(buffer))
	}

	/// Reads all the data from the file.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	async fn read_all(&self) -> Result<Blob> {
		let mut buffer = vec![0; self.size as usize];
		let mut file = self
			.file
			.try_clone()
			.with_context(|| format!("failed to clone file '{}'", self.name))?;
		file
			.seek(SeekFrom::Start(0))
			.with_context(|| format!("failed to seek to start of file '{}'", self.name))?;
		file
			.read_exact(&mut buffer)
			.with_context(|| format!("failed to read all {} bytes from file '{}'", self.size, self.name))?;
		Ok(Blob::from(buffer))
	}

	/// Gets the name of the data source.
	///
	/// # Returns
	///
	/// * A string slice representing the name of the data source.
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl Read for DataReaderFile {
	/// Reads data into the provided buffer.
	///
	/// # Arguments
	///
	/// * `buf` - A mutable byte slice to read data into.
	///
	/// # Returns
	///
	/// * The number of bytes read or an error.
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		self.file.read(buf)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::assert_wildcard;
	use assert_fs::NamedTempFile;
	use std::io::Write;

	// Test the 'new' method for valid and invalid files
	#[tokio::test]
	async fn new() -> Result<()> {
		let temp_file_path = NamedTempFile::new("testfile.txt")?;
		let invalid_path = NamedTempFile::new("nonexistent.txt")?;

		// Create a temporary file
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?;
		}

		// Test with a valid file path
		let data_reader_file = DataReaderFile::open(temp_file_path.path());
		assert!(data_reader_file.is_ok());

		// Test with an invalid file path
		let data_reader_file = DataReaderFile::open(invalid_path.path());
		assert!(data_reader_file.is_err());

		Ok(())
	}

	// Test the 'read_range' method
	#[tokio::test]
	async fn read_range() -> Result<()> {
		let temp_file_path = NamedTempFile::new("testfile.txt")?;

		// Create a temporary file
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?;
		}

		let data_reader_file = DataReaderFile::open(temp_file_path.path())?;

		// Define a range to read
		let range = ByteRange { offset: 4, length: 6 };

		// Read the specified range from the file
		let blob = data_reader_file.read_range(&range).await?;

		// Check if the read range matches the expected text
		assert_eq!(blob.as_str(), "o, wor");

		Ok(())
	}

	// Test the 'get_name' method
	#[tokio::test]
	async fn get_name() -> Result<()> {
		let temp_file_path = NamedTempFile::new("testfile.txt")?;

		// Create a temporary file
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?;
		}

		let data_reader_file = DataReaderFile::open(temp_file_path.path())?;

		// Check if the name matches the original file path
		assert_wildcard!(data_reader_file.get_name(), "*testfile.txt");

		Ok(())
	}

	// Test the synchronous `Read` implementation
	#[test]
	fn read_sync_and_read_trait() -> Result<()> {
		let temp_file = NamedTempFile::new("testfile_sync.txt")?;
		// Write data to the file
		{
			let mut f = File::create(temp_file.path())?;
			f.write_all(b"Sync read test")?;
		}

		let mut reader = DataReaderFile::open(temp_file.path()).unwrap();
		let mut buf = Vec::new();
		// Box<DataReaderFile> implements Read
		reader.read_to_end(&mut buf)?;
		assert_eq!(buf, b"Sync read test");
		Ok(())
	}

	// Test the `read_all` async method
	#[tokio::test]
	async fn test_read_all_method() -> Result<()> {
		let temp_file = NamedTempFile::new("testfile_all.txt")?;
		// Write data to the file
		{
			let mut f = File::create(temp_file.path())?;
			f.write_all(b"Async read all test")?;
		}

		let reader = DataReaderFile::open(temp_file.path())?;
		let blob = reader.read_all().await?;
		assert_eq!(blob.as_slice(), b"Async read all test");
		Ok(())
	}
}
