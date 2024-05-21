#![allow(dead_code)]

use crate::types::Blob;

use super::ValueWriter;
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	io::{Cursor, Write},
	marker::PhantomData,
};

pub struct ValueWriterBlob<E: ByteOrder> {
	_phantom: PhantomData<E>,
	cursor: Cursor<Vec<u8>>,
}

impl<E: ByteOrder> ValueWriterBlob<E> {
	pub fn new() -> ValueWriterBlob<E> {
		ValueWriterBlob {
			_phantom: PhantomData,
			cursor: Cursor::new(Vec::new()),
		}
	}

	pub fn into_blob(self) -> Blob {
		Blob::from(self.cursor.into_inner())
	}
}

impl ValueWriterBlob<LittleEndian> {
	pub fn new_le() -> ValueWriterBlob<LittleEndian> {
		ValueWriterBlob::new()
	}
}

impl ValueWriterBlob<BigEndian> {
	pub fn new_be() -> ValueWriterBlob<BigEndian> {
		ValueWriterBlob::new()
	}
}

impl<E: ByteOrder> ValueWriter<E> for ValueWriterBlob<E> {
	fn get_writer(&mut self) -> &mut dyn Write {
		&mut self.cursor
	}

	fn position(&mut self) -> Result<u64> {
		Ok(self.cursor.position())
	}
}

impl<E: ByteOrder> Default for ValueWriterBlob<E> {
	fn default() -> Self {
		Self::new()
	}
}
