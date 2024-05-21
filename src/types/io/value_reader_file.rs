#![allow(dead_code)]

use super::{SeekRead, ValueReader, ValueReaderBlob};
use crate::types::Blob;
use anyhow::{anyhow, bail, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	fs::File,
	io::{BufReader, Cursor, Read, Seek, SeekFrom},
	marker::PhantomData,
	sync::Arc,
};

// Define the ValueReaderFile struct
pub struct ValueReaderFile<E: ByteOrder> {
	_phantom: PhantomData<E>,
	reader: BufReader<File>,
	len: u64,
}

impl<E: ByteOrder> ValueReaderFile<E> {
	pub fn new(file: File) -> Result<ValueReaderFile<E>> {
		let len = file.metadata()?.len();
		Ok(ValueReaderFile {
			_phantom: PhantomData,
			reader: BufReader::new(file),
			len,
		})
	}
}

impl SeekRead for BufReader<File> {}

impl<E: ByteOrder> ValueReaderFile<E> {
	pub fn new_le(file: File) -> Result<ValueReaderFile<LittleEndian>> {
		ValueReaderFile::new(file)
	}

	pub fn new_be(file: File) -> Result<ValueReaderFile<BigEndian>> {
		ValueReaderFile::new(file)
	}
}

impl<'a, E: ByteOrder + 'a> ValueReader<'a, E> for ValueReaderFile<E> {
	fn get_reader(&mut self) -> &mut dyn SeekRead {
		&mut self.reader
	}

	fn len(&self) -> u64 {
		self.len
	}

	fn position(&mut self) -> u64 {
		self.reader.stream_position().unwrap()
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		if position >= self.len {
			bail!("set position outside length")
		}
		self.reader.seek(SeekFrom::Start(position))?;
		Ok(())
	}

	fn get_sub_reader<'b>(&'b mut self, length: u64) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b,
	{
		let start = self.reader.stream_position()?;
		let end = start + length;
		if end > self.len {
			bail!("sub-reader length exceeds file length");
		}

		let mut buffer = vec![0; length as usize];
		self.reader.read_exact(&mut buffer)?;

		Ok(Box::new(ValueReaderBlob::<E>::new(Blob::from(buffer))))
	}
}
