#![allow(dead_code)]

use super::{types::DataReaderTrait, DataWriterBlob};
use crate::types::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::io::{Cursor, Read, Seek, SeekFrom};

#[derive(Debug)]
pub struct DataReaderBlob {
	reader: Cursor<Vec<u8>>,
}

impl DataReaderBlob {
	pub fn len(&self) -> usize {
		self.reader.get_ref().len()
	}
	pub fn is_empty(&self) -> bool {
		self.reader.get_ref().len() == 0
	}
}

#[async_trait]
impl DataReaderTrait for DataReaderBlob {
	async fn read_range(&mut self, range: &ByteRange) -> Result<Blob> {
		let mut buffer = vec![0; range.length as usize];

		self.reader.seek(SeekFrom::Start(range.offset))?;
		self.reader.read_exact(&mut buffer)?;

		Ok(Blob::from(buffer))
	}
	fn get_name(&self) -> &str {
		"memory"
	}
}

impl Read for DataReaderBlob {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		self.reader.read(buf)
	}
}

impl From<Box<DataWriterBlob>> for DataReaderBlob {
	fn from(value: Box<DataWriterBlob>) -> Self {
		DataReaderBlob::from(value.into_blob())
	}
}

impl From<DataWriterBlob> for DataReaderBlob {
	fn from(value: DataWriterBlob) -> Self {
		DataReaderBlob::from(value.into_blob())
	}
}

impl From<Blob> for DataReaderBlob {
	fn from(value: Blob) -> Self {
		DataReaderBlob {
			reader: Cursor::new(value.into_vec()),
		}
	}
}

impl From<Vec<u8>> for DataReaderBlob {
	fn from(value: Vec<u8>) -> Self {
		DataReaderBlob {
			reader: Cursor::new(value),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::io::types::DataWriterTrait;

	#[tokio::test]
	async fn from_blob() -> Result<()> {
		let blob = Blob::from(vec![0, 1, 2, 3, 4, 5, 6, 7]);

		let mut data_reader = DataReaderBlob::from(blob.clone());

		assert_eq!(data_reader.get_name(), "memory");

		assert_eq!(data_reader.read_range(&ByteRange::new(0, 8)).await?, blob);

		assert_eq!(
			data_reader.read_range(&ByteRange::new(0, 4)).await?.as_slice(),
			&blob.as_slice()[0..4]
		);

		assert!(data_reader.read_range(&ByteRange::new(0, 9)).await.is_err());

		Ok(())
	}

	#[tokio::test]
	async fn from_vec() -> Result<()> {
		let data = vec![10, 20, 30, 40, 50, 60, 70, 80];
		let mut data_reader = DataReaderBlob::from(data.clone());

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
		let mut data_reader: DataReaderBlob = data_writer.into();

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
		let mut data_reader: DataReaderBlob = Box::new(data_writer).into();

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
		let mut data_reader = DataReaderBlob::from(data);

		// Range starts at the end of the data
		assert!(data_reader.read_range(&ByteRange::new(8, 1)).await.is_err());

		// Range starts within the data but exceeds its length
		assert!(data_reader.read_range(&ByteRange::new(6, 3)).await.is_err());

		Ok(())
	}
}
