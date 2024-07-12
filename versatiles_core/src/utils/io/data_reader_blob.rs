//! This module provides functionality for reading data from in-memory blobs.
//!
//! # Overview
//!
//! The `DataReaderBlob` struct allows for reading data stored in an in-memory
//! vector of bytes (`Vec<u8>`). It implements the `DataReaderTrait` to provide
//! asynchronous reading capabilities and the standard library's `Read` trait for
//! synchronous reading.
//!
//! # Examples
//!
//! ```rust
//! use versatiles::{utils::io::{DataReaderBlob, DataReaderTrait}, types::{Blob, ByteRange}};
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let data = vec![1, 2, 3, 4, 5];
//!     let mut reader = DataReaderBlob::from(data);
//!     
//!     // Reading all data
//!     let all_data = reader.read_all().await?;
//!     assert_eq!(all_data.as_slice(), &[1, 2, 3, 4, 5]);
//!
//!     // Reading a range of data
//!     let range = ByteRange::new(1, 3);
//!     let partial_data = reader.read_range(&range).await?;
//!     assert_eq!(partial_data.as_slice(), &[2, 3, 4]);
//!
//!     Ok(())
//! }
//! ```

#![allow(dead_code)]

use super::{DataReaderTrait, DataWriterBlob};
use crate::types::{Blob, ByteRange};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use std::io::{Cursor, Read};

/// A struct that provides reading capabilities from an in-memory blob of data.
#[derive(Debug)]
pub struct DataReaderBlob {
	blob: Cursor<Vec<u8>>,
}

impl DataReaderBlob {
	/// Returns the length of the data in the reader.
	pub fn len(&self) -> usize {
		self.blob.get_ref().len()
	}

	/// Checks if the reader is empty.
	pub fn is_empty(&self) -> bool {
		self.blob.get_ref().len() == 0
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderBlob {
	/// Reads a specific range of bytes from the data.
	///
	/// # Arguments
	///
	/// * `range` - A ByteRange struct specifying the offset and length of the range to read.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with the read data or an error.
	async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		let start = range.offset as usize;
		let end = (range.offset + range.length) as usize;
		let blob = self.blob.get_ref();
		ensure!(
			end <= blob.len(),
			"end of range ({start}..{end}) is outside blob ({})",
			blob.len()
		);
		Ok(Blob::from(&blob[start..end]))
	}

	/// Reads all the data from the reader.
	///
	/// # Returns
	///
	/// * A Result containing a Blob with all the data or an error.
	async fn read_all(&self) -> Result<Blob> {
		Ok(Blob::from(self.blob.get_ref()))
	}

	/// Gets the name of the data source.
	///
	/// # Returns
	///
	/// * A string slice representing the name of the data source.
	fn get_name(&self) -> &str {
		"memory"
	}
}

impl Read for DataReaderBlob {
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
		self.blob.read(buf)
	}
}

impl From<Box<DataWriterBlob>> for DataReaderBlob {
	/// Creates a DataReaderBlob from a boxed DataWriterBlob.
	///
	/// # Arguments
	///
	/// * `value` - A boxed DataWriterBlob.
	///
	/// # Returns
	///
	/// * A new DataReaderBlob.
	fn from(value: Box<DataWriterBlob>) -> Self {
		DataReaderBlob::from(value.into_blob())
	}
}

impl From<DataWriterBlob> for DataReaderBlob {
	/// Creates a DataReaderBlob from a DataWriterBlob.
	///
	/// # Arguments
	///
	/// * `value` - A DataWriterBlob.
	///
	/// # Returns
	///
	/// * A new DataReaderBlob.
	fn from(value: DataWriterBlob) -> Self {
		DataReaderBlob::from(value.into_blob())
	}
}

impl From<Blob> for DataReaderBlob {
	/// Creates a DataReaderBlob from a Blob.
	///
	/// # Arguments
	///
	/// * `value` - A Blob.
	///
	/// # Returns
	///
	/// * A new DataReaderBlob.
	fn from(value: Blob) -> Self {
		DataReaderBlob {
			blob: Cursor::new(value.into_vec()),
		}
	}
}

impl From<Vec<u8>> for DataReaderBlob {
	/// Creates a DataReaderBlob from a vector of bytes.
	///
	/// # Arguments
	///
	/// * `value` - A vector of bytes.
	///
	/// # Returns
	///
	/// * A new DataReaderBlob.
	fn from(value: Vec<u8>) -> Self {
		DataReaderBlob {
			blob: Cursor::new(value),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{super::DataWriterTrait, *};

	#[tokio::test]
	async fn from_blob() -> Result<()> {
		let blob = Blob::from(vec![0, 1, 2, 3, 4, 5, 6, 7]);

		let data_reader = DataReaderBlob::from(blob.clone());

		assert_eq!(data_reader.get_name(), "memory");

		assert_eq!(data_reader.read_range(&ByteRange::new(0, 8)).await?, blob);

		assert_eq!(
			data_reader
				.read_range(&ByteRange::new(0, 4))
				.await?
				.as_slice(),
			&blob.as_slice()[0..4]
		);

		assert!(data_reader.read_range(&ByteRange::new(0, 9)).await.is_err());

		Ok(())
	}

	#[tokio::test]
	async fn from_vec() -> Result<()> {
		let data = vec![10, 20, 30, 40, 50, 60, 70, 80];
		let data_reader = DataReaderBlob::from(data.clone());

		assert_eq!(data_reader.get_name(), "memory");
		assert_eq!(data_reader.len(), data.len());

		let range = ByteRange::new(2, 4);
		let result = data_reader.read_range(&range).await?;
		assert_eq!(result.as_slice(), &data[2..6]);

		Ok(())
	}

	#[tokio::test]
	async fn from_data_writer_blob() -> Result<()> {
		let data = [100u8, 101, 102, 103, 104, 105].as_slice();
		let mut data_writer = DataWriterBlob::new()?;
		data_writer.append(&Blob::from(data))?;
		let data_reader: DataReaderBlob = data_writer.into();

		assert_eq!(data_reader.get_name(), "memory");
		assert_eq!(data_reader.len(), 6);

		let range = ByteRange::new(0, 6);
		let result = data_reader.read_range(&range).await?;
		assert_eq!(result.as_slice(), data);

		Ok(())
	}

	#[tokio::test]
	async fn from_boxed_data_writer_blob() -> Result<()> {
		let data = [100u8, 101, 102, 103, 104, 105].as_slice();
		let mut data_writer = DataWriterBlob::new()?;
		data_writer.append(&Blob::from(data))?;
		let data_reader: DataReaderBlob = Box::new(data_writer).into();

		assert_eq!(data_reader.get_name(), "memory");
		assert_eq!(data_reader.len(), 6);

		let range = ByteRange::new(0, 6);
		let result = data_reader.read_range(&range).await?;
		assert_eq!(result.as_slice(), data);

		Ok(())
	}

	#[tokio::test]
	async fn test_len() {
		let data = vec![1, 2, 3, 4, 5];
		let data_reader = DataReaderBlob::from(data.clone());
		assert_eq!(data_reader.len(), data.len());
	}

	#[tokio::test]
	async fn test_read() {
		let data = vec![1, 2, 3, 4, 5];
		let mut data_reader = DataReaderBlob::from(data);

		let mut buffer = [0; 3];
		let n = data_reader.read(&mut buffer).unwrap();
		assert_eq!(n, 3);
		assert_eq!(buffer, [1, 2, 3]);

		let n = data_reader.read(&mut buffer).unwrap();
		assert_eq!(n, 2);
		assert_eq!(&buffer[..2], &[4, 5]);
	}

	// Test edge cases for 'read_range' method
	#[tokio::test]
	async fn test_read_range_edge_cases() -> Result<()> {
		let data = vec![10, 20, 30, 40, 50, 60, 70, 80];
		let data_reader = DataReaderBlob::from(data);

		// Range starts at the end of the data
		assert!(data_reader.read_range(&ByteRange::new(8, 1)).await.is_err());

		// Range starts within the data but exceeds its length
		assert!(data_reader.read_range(&ByteRange::new(6, 3)).await.is_err());

		Ok(())
	}
}
