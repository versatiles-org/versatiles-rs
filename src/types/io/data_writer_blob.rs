#![allow(dead_code)]

use super::{types::DataWriterTrait, DataReaderBlob};
use crate::types::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::io::{Cursor, Seek, SeekFrom, Write};

#[derive(Clone)]
pub struct DataWriterBlob {
	writer: Cursor<Vec<u8>>,
}

impl DataWriterBlob {
	pub fn new() -> Result<DataWriterBlob> {
		Ok(DataWriterBlob {
			writer: Cursor::new(Vec::new()),
		})
	}
	pub fn as_slice(&self) -> &[u8] {
		self.writer.get_ref().as_slice()
	}
	pub fn into_blob(self) -> Blob {
		Blob::from(self.writer.into_inner())
	}
	pub fn into_reader(self) -> DataReaderBlob {
		DataReaderBlob::from(self)
	}
	pub fn to_reader(&self) -> DataReaderBlob {
		DataReaderBlob::from(self.writer.get_ref().clone())
	}
	pub fn len(&self) -> usize {
		self.writer.get_ref().len()
	}
}

#[async_trait]
impl DataWriterTrait for DataWriterBlob {
	fn append(&mut self, blob: &Blob) -> Result<ByteRange> {
		let pos = self.writer.stream_position()?;
		let len = self.writer.write(blob.as_slice())?;

		Ok(ByteRange::new(pos, len as u64))
	}

	fn write_start(&mut self, blob: &Blob) -> Result<()> {
		let pos = self.writer.stream_position()?;
		self.writer.rewind()?;
		self.writer.write_all(blob.as_slice())?;
		self.writer.seek(SeekFrom::Start(pos))?;
		Ok(())
	}

	fn get_position(&mut self) -> Result<u64> {
		Ok(self.writer.stream_position()?)
	}
	fn set_position(&mut self, position: u64) -> Result<()> {
		self.writer.seek(SeekFrom::Start(position))?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::{Blob, DataReaderTrait};

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
