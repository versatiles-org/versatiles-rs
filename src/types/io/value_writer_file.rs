#![allow(dead_code)]

use super::ValueWriter;
use crate::types::Blob;
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	fs::File,
	io::{BufWriter, Cursor, Seek, Write},
	marker::PhantomData,
};

pub struct ValueWriterFile<E: ByteOrder> {
	_phantom: PhantomData<E>,
	writer: BufWriter<File>,
}

impl<E: ByteOrder> ValueWriterFile<E> {
	pub fn new(file: File) -> ValueWriterFile<E> {
		ValueWriterFile {
			_phantom: PhantomData,
			writer: BufWriter::new(file),
		}
	}
}

impl ValueWriterFile<LittleEndian> {
	pub fn new_le(file: File) -> ValueWriterFile<LittleEndian> {
		ValueWriterFile::new(file)
	}
}

impl ValueWriterFile<BigEndian> {
	pub fn new_be(file: File) -> ValueWriterFile<BigEndian> {
		ValueWriterFile::new(file)
	}
}

impl<E: ByteOrder> ValueWriter<E> for ValueWriterFile<E> {
	fn get_writer(&mut self) -> &mut dyn Write {
		&mut self.writer
	}

	fn position(&mut self) -> Result<u64> {
		Ok(self.writer.stream_position()?)
	}
}
