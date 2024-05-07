use super::super::types::ByteRange;
use super::DataWriterTrait;
use crate::shared::Blob;
use anyhow::{ensure, Result};
use async_trait::async_trait;
use std::{
	fs::File,
	io::{BufWriter, Seek, SeekFrom, Write},
	path::Path,
};

pub struct DataWriterFile {
	writer: BufWriter<File>,
}

#[async_trait]
impl DataWriterTrait for DataWriterFile {
	fn new(path: &Path) -> Result<Box<Self>> {
		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		Ok(Box::new(DataWriterFile {
			writer: BufWriter::new(File::create(path)?),
		}))
	}

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
}
