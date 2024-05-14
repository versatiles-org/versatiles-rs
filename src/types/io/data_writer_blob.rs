#![allow(dead_code)]

use super::{types::DataWriterTrait, DataReaderBlob};
use crate::types::{Blob, ByteRange};
use anyhow::Result;
use async_trait::async_trait;
use std::io::{Cursor, Seek, SeekFrom, Write};

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
