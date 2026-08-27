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

use std::{fs::File, io::Read, path::Path};

use anyhow::{Result, ensure};
use async_trait::async_trait;
use versatiles_derive::context;

use super::DataReaderTrait;
use crate::{Blob, ByteRange};

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
	#[context("while opening file {path:?}")]
	pub fn open(path: &Path) -> Result<Box<DataReaderFile>> {
		ensure!(path.exists(), "file {path:?} does not exist");
		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.is_file(), "path {path:?} must be a file");

		let path = path.canonicalize()?;
		let file = File::open(&path)?;
		let size = file.metadata()?.len();

		Ok(Box::new(DataReaderFile {
			name: path.to_string_lossy().into_owned(),
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
	///
	/// # Thread Safety
	///
	/// This method uses position-independent I/O (`pread` on Unix, `seek_read` on Windows)
	/// which is thread-safe and allows concurrent reads without race conditions.
	#[context("while reading range {range:?} from file '{}'", self.name)]
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		// Checked before the allocation, not after. Container headers carry byte
		// ranges as raw u64s, so a 127-byte crafted PMTiles file could ask for a
		// four-exabyte read: `vec![0; n]` then aborts the process through
		// `handle_alloc_error`, which no caller can catch. Comparing against the
		// file size first turns that into an ordinary error.
		let end = range
			.offset
			.checked_add(range.length)
			.context("range offset + length overflows u64")?;
		ensure!(
			end <= self.size,
			"range {range} is outside file '{}', which is {} bytes long",
			self.name,
			self.size
		);

		let mut buffer = vec![0; usize::try_from(range.length).context("Range length too large for this platform")?];

		// Use position-independent I/O - thread-safe by design
		#[cfg(unix)]
		{
			use std::os::unix::fs::FileExt;
			self.file.read_exact_at(&mut buffer, range.offset).with_context(|| {
				format!(
					"failed to read {} bytes at offset {} in file '{}'",
					range.length, range.offset, self.name
				)
			})?;
		}

		#[cfg(windows)]
		{
			use std::os::windows::fs::FileExt;
			self.file.seek_read(&mut buffer, range.offset).with_context(|| {
				format!(
					"failed to read {} bytes at offset {} in file '{}'",
					range.length, range.offset, self.name
				)
			})?;
		}

		#[cfg(not(any(unix, windows)))]
		{
			compile_error!("Platform not supported: need position-independent file I/O");
		}

		Ok(Blob::from(buffer))
	}

	/// Reads all the data from the file.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	///
	/// # Thread Safety
	///
	/// This method uses position-independent I/O which is thread-safe.
	#[context("while reading all bytes from file '{}'", self.name)]
	async fn read_all(&self) -> Result<Blob> {
		let mut buffer = vec![0; usize::try_from(self.size).context("File size too large for this platform")?];

		#[cfg(unix)]
		{
			use std::os::unix::fs::FileExt;
			self
				.file
				.read_exact_at(&mut buffer, 0)
				.with_context(|| format!("failed to read all {} bytes from file '{}'", self.size, self.name))?;
		}

		#[cfg(windows)]
		{
			use std::os::windows::fs::FileExt;
			self
				.file
				.seek_read(&mut buffer, 0)
				.with_context(|| format!("failed to read all {} bytes from file '{}'", self.size, self.name))?;
		}

		Ok(Blob::from(buffer))
	}

	/// Gets the name of the data source.
	///
	/// # Returns
	///
	/// * A string slice representing the name of the data source.
	fn name(&self) -> &str {
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
	use std::io::Write;

	use assert_fs::NamedTempFile;

	use super::*;
	use crate::assert_wildcard;

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

	/// A range that reaches past the end of the file is rejected before the
	/// buffer for it is allocated. Container headers hold byte ranges as raw
	/// u64s, so this is what a crafted file asks for; allocating first meant
	/// `handle_alloc_error` aborted the process instead of returning an error.
	#[tokio::test]
	async fn read_range_outside_file_is_an_error() -> Result<()> {
		let temp_file_path = NamedTempFile::new("bounds.txt")?;
		{
			let mut temp_file = File::create(&temp_file_path)?;
			temp_file.write_all(b"Hello, world!")?; // 13 bytes
		}
		let reader = DataReaderFile::open(temp_file_path.path())?;

		// The whole file still reads.
		assert_eq!(
			reader.read_range(&ByteRange::new(0, 13)).await?.as_str(),
			"Hello, world!"
		);

		// One byte too many, an absurd length, and an offset past the end.
		assert!(reader.read_range(&ByteRange::new(0, 14)).await.is_err());
		assert!(reader.read_range(&ByteRange::new(13, 1)).await.is_err());
		assert!(reader.read_range(&ByteRange::new(0, 1 << 62)).await.is_err());

		// offset + length overflowing u64 is an error, not a wrap.
		assert!(reader.read_range(&ByteRange::new(u64::MAX, 1)).await.is_err());

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
		assert_wildcard!(data_reader_file.name(), "*testfile.txt");

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

	// Test concurrent range reads to verify thread safety
	#[tokio::test]
	async fn concurrent_range_reads_return_correct_data() -> Result<()> {
		use std::sync::Arc;

		use tokio::task::JoinSet;

		// Create test file with known pattern
		let temp_file = NamedTempFile::new("concurrent_test.dat")?;
		let mut file = File::create(&temp_file)?;

		// Write 10KB of data with predictable pattern
		// Each byte is (position % 256) so we can verify correctness
		let mut data = Vec::new();
		for i in 0u16..10240 {
			// Safe: i is non-negative and i & 0xFF always fits in u8
			data.push((i & 0xFF) as u8);
		}
		file.write_all(&data)?;
		drop(file);

		let reader = Arc::new(DataReaderFile::open(temp_file.path())?);

		// Spawn 100 concurrent tasks reading different ranges
		let mut join_set = JoinSet::new();

		for task_id in 0..100 {
			let reader_clone = Arc::clone(&reader);

			join_set.spawn(async move {
				// Each task reads a different 100-byte range
				let offset = (task_id * 100) % 10000;
				let length = 100;
				let range = ByteRange::new(offset, length);

				let blob = reader_clone.read_range(&range).await?;

				// Verify every byte is correct
				for (i, &byte) in blob.as_slice().iter().enumerate() {
					let expected = ((offset + i as u64) & 0xFF) as u8;
					assert_eq!(
						byte,
						expected,
						"Task {}: Byte {} at offset {} is {} but expected {}",
						task_id,
						i,
						offset + i as u64,
						byte,
						expected
					);
				}

				Ok::<_, anyhow::Error>(())
			});
		}

		// Wait for all tasks and check results
		while let Some(result) = join_set.join_next().await {
			result??;
		}

		Ok(())
	}
}
