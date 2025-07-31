//! This module provides functionality for writing data to files.
//!
//! # Overview
//!
//! The `DataWriterFile` struct allows for writing data to files on the filesystem.
//! It implements the `DataWriterTrait` to provide methods for appending data, writing data from the start,
//! and managing the write position. The module ensures the file path is absolute before attempting to create or write to the file.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::{DataWriterFile, DataWriterTrait}, Blob, ByteRange};
//! use anyhow::Result;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let path = std::env::current_dir()?.join("../testdata/temp.txt");
//!     let mut writer = DataWriterFile::from_path(&path)?;
//!     let data = Blob::from(vec![1, 2, 3, 4]);
//!
//!     // Appending data
//!     writer.append(&data)?;
//!     assert_eq!(writer.get_position()?, 4);
//!
//!     // Writing data from the start
//!     writer.write_start(&Blob::from(vec![5, 6, 7, 8]))?;
//!     writer.set_position(0)?;
//!     assert_eq!(writer.get_position()?, 0);
//!
//!     Ok(())
//! }
//! ```

use super::DataWriterTrait;
use crate::{Blob, ByteRange};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{
	fs::File,
	io::{BufWriter, Seek, SeekFrom, Write},
	path::Path,
};

/// A struct that provides writing capabilities to a file.
pub struct DataWriterFile {
	writer: BufWriter<File>,
}

impl DataWriterFile {
	/// Creates a `DataWriterFile` from a file path.
	///
	/// # Arguments
	///
	/// * `path` - A reference to the file path to create and write to.
	///
	/// # Returns
	///
	/// * A Result containing the new `DataWriterFile` instance or an error.
	pub fn from_path(path: &Path) -> Result<DataWriterFile> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		Ok(DataWriterFile {
			writer: BufWriter::new(File::create(path)?),
		})
	}
}

#[async_trait]
impl DataWriterTrait for DataWriterFile {
	/// Appends data to the file.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to append.
	///
	/// # Returns
	///
	/// * A Result containing a `ByteRange` indicating the position and length of the appended data, or an error.
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.writer.stream_position()?;
		let len = self.writer.write(blob.as_slice())?;

		Ok(ByteRange::new(pos, len as u64))
	}

	/// Writes data from the start of the file.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to write.
	///
	/// # Returns
	///
	/// * A Result indicating success or an error.
	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		let pos = self.writer.stream_position()?;
		self.writer.rewind()?;
		self.writer.write_all(blob.as_slice())?;
		self.writer.seek(SeekFrom::Start(pos))?;
		Ok(())
	}

	/// Gets the current write position.
	///
	/// # Returns
	///
	/// * A Result containing the current write position in bytes or an error.
	fn get_position(&mut self) -> Result<u64> {
		Ok(self.writer.stream_position()?)
	}

	/// Sets the write position.
	///
	/// # Arguments
	///
	/// * `position` - The position to set in bytes.
	///
	/// # Returns
	///
	/// * A Result indicating success or an error.
	fn set_position(&mut self, position: u64) -> Result<()> {
		self.writer.seek(SeekFrom::Start(position))?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Blob;
	use anyhow::Result;
	use assert_fs::NamedTempFile;
	use std::fs::File;
	use std::io::Read;

	#[test]
	fn test_append_and_get_position() -> Result<()> {
		// Create a temporary file
		let temp = NamedTempFile::new("test1")?;
		let path = temp.path();
		// Ensure absolute path
		assert!(path.is_absolute());

		let mut writer = DataWriterFile::from_path(path)?;
		let data = Blob::from(vec![10, 20, 30]);
		// Append data
		let range = writer.append(&data)?;
		assert_eq!(range.to_string(), "[0..2]");
		// Position should now equal length
		assert_eq!(writer.get_position()?, 3);

		// Read back file contents
		let mut file = File::open(path)?;
		let mut buf = Vec::new();
		file.read_to_end(&mut buf)?;
		assert_eq!(buf, data.as_slice());
		Ok(())
	}

	#[test]
	fn test_write_start_and_append() -> Result<()> {
		let temp = NamedTempFile::new("test2")?;
		let path = temp.path();
		let mut writer = DataWriterFile::from_path(path)?;

		// Write start should write data at position 0
		let start = Blob::from(vec![1, 2, 3, 4]);
		writer.write_start(&start)?;
		// After write_start, position unchanged (0)
		assert_eq!(writer.get_position()?, 0);

		// Now append more data
		let extra = Blob::from(vec![5, 6]);
		let range2 = writer.append(&extra)?;
		assert_eq!(range2.to_string(), "[0..1]");

		drop(writer);

		// Read back file contents
		let mut file = File::open(path)?;
		let mut buf = Vec::new();
		file.read_to_end(&mut buf)?;
		// File should contain extra data because append writes at offset 0 after write_start
		assert_eq!(buf, &[5, 6, 3, 4]);
		Ok(())
	}
}
