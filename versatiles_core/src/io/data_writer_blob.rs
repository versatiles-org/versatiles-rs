//! This module provides functionality for writing data to in-memory blobs.
//!
//! # Overview
//!
//! The `DataWriterBlob` struct allows for writing data to an in-memory vector of bytes (`Vec<u8>`).
//! It implements the `DataWriterTrait` to provide methods for appending data, writing data from the start,
//! and managing the write position. Additionally, it provides methods to convert the writer into a reader
//! or a `Blob` for further processing.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::{DataWriterBlob, DataWriterTrait}, Blob, ByteRange};
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let mut writer = DataWriterBlob::new()?;
//!     let data = Blob::from(vec![1, 2, 3, 4]);
//!
//!     // Appending data
//!     writer.append(&data)?;
//!     assert_eq!(writer.as_slice(), &[1, 2, 3, 4]);
//!
//!     // Writing data from the start
//!     writer.write_start(&Blob::from(vec![5, 6, 7, 8]))?;
//!     assert_eq!(writer.as_slice(), &[5, 6, 7, 8]);
//!
//!     Ok(())
//! }
//! ```

#![allow(dead_code)]

use super::{DataReaderBlob, DataWriterTrait};
use crate::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::io::{Cursor, Seek, SeekFrom, Write};

/// A struct that provides writing capabilities to an in-memory blob of data.
#[derive(Clone)]
pub struct DataWriterBlob {
	writer: Cursor<Vec<u8>>,
}

impl DataWriterBlob {
	/// Creates a new `DataWriterBlob` instance.
	///
	/// # Returns
	///
	/// * A Result containing the new `DataWriterBlob` instance or an error.
	pub fn new() -> Result<DataWriterBlob> {
		Ok(DataWriterBlob {
			writer: Cursor::new(Vec::new()),
		})
	}

	/// Returns the data as a slice.
	///
	/// # Returns
	///
	/// * A byte slice of the data.
	pub fn as_slice(&self) -> &[u8] {
		self.writer.get_ref().as_slice()
	}

	/// Converts the writer into a `Blob`.
	///
	/// # Returns
	///
	/// * A `Blob` containing the data.
	pub fn into_blob(self) -> Blob {
		Blob::from(self.writer.into_inner())
	}

	/// Converts the writer into a `DataReaderBlob`.
	///
	/// # Returns
	///
	/// * A `DataReaderBlob` instance.
	pub fn into_reader(self) -> DataReaderBlob {
		DataReaderBlob::from(self)
	}

	/// Creates a `DataReaderBlob` from the current state of the writer.
	///
	/// # Returns
	///
	/// * A `DataReaderBlob` instance.
	pub fn to_reader(&self) -> DataReaderBlob {
		DataReaderBlob::from(self.writer.get_ref().clone())
	}

	/// Returns the length of the data.
	///
	/// # Returns
	///
	/// * The length of the data in bytes.
	pub fn len(&self) -> usize {
		self.writer.get_ref().len()
	}

	/// Checks if the writer is empty.
	///
	/// # Returns
	///
	/// * `true` if the writer is empty, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.writer.get_ref().len() == 0
	}
}

#[async_trait]
impl DataWriterTrait for DataWriterBlob {
	/// Appends data to the writer.
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

	/// Writes data from the start of the writer.
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
	use super::super::DataReaderTrait;
	use super::*;

	#[test]
	fn test_new() -> Result<()> {
		let writer = DataWriterBlob::new()?;
		assert_eq!(writer.len(), 0);
		Ok(())
	}

	#[test]
	fn test_append() -> Result<()> {
		let mut writer = DataWriterBlob::new()?;
		let blob = Blob::from(vec![1, 2, 3, 4]);

		let range = writer.append(&blob)?;
		assert_eq!(range, ByteRange::new(0, 4));
		assert_eq!(writer.as_slice(), blob.as_slice());

		let more_data = Blob::from(vec![5, 6, 7, 8]);
		let range = writer.append(&more_data)?;
		assert_eq!(range, ByteRange::new(4, 4));
		assert_eq!(writer.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8]);

		Ok(())
	}

	#[test]
	fn test_write_start() -> Result<()> {
		let mut writer = DataWriterBlob::new()?;
		let blob = Blob::from(vec![1, 2, 3, 4]);

		writer.append(&blob)?;
		writer.write_start(&Blob::from(vec![5, 6, 7, 8]))?;

		assert_eq!(writer.as_slice(), &[5, 6, 7, 8]);

		Ok(())
	}

	#[test]
	fn test_get_and_set_position() -> Result<()> {
		let mut writer = DataWriterBlob::new()?;
		writer.append(&Blob::from(vec![1, 2, 3, 4]))?;

		assert_eq!(writer.get_position()?, 4);
		writer.set_position(2)?;
		assert_eq!(writer.get_position()?, 2);

		writer.append(&Blob::from(vec![5, 6]))?;
		assert_eq!(writer.as_slice(), &[1, 2, 5, 6]);

		Ok(())
	}

	#[test]
	fn test_as_slice() -> Result<()> {
		let mut writer = DataWriterBlob::new()?;
		let blob = Blob::from(vec![1, 2, 3, 4]);

		writer.append(&blob)?;
		assert_eq!(writer.as_slice(), blob.as_slice());

		Ok(())
	}

	#[tokio::test]
	async fn test_into_reader_blob() -> Result<()> {
		let mut writer = DataWriterBlob::new()?;
		let blob = Blob::from(vec![1, 2, 3, 4]);
		let range = ByteRange::new(0, 4);

		writer.append(&blob)?;

		assert_eq!(writer.clone().to_reader().read_range(&range).await?, blob);

		assert_eq!(writer.clone().into_reader().read_range(&range).await?, blob);

		assert_eq!(writer.clone().into_blob(), blob);

		Ok(())
	}

	#[test]
	fn test_len() -> Result<()> {
		let mut writer = DataWriterBlob::new()?;
		let blob = Blob::from(vec![1, 2, 3, 4]);

		writer.append(&blob)?;
		assert_eq!(writer.len(), 4);

		let more_data = Blob::from(vec![5, 6, 7, 8]);
		writer.append(&more_data)?;
		assert_eq!(writer.len(), 8);

		Ok(())
	}
}
