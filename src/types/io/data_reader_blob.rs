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

	// Test the 'from_blob' method
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
}
