use crate::types::Blob;
use anyhow::Result;
use byteorder::{LittleEndian as LE, WriteBytesExt};
use std::io::{Cursor, Write};

pub struct BlobWriter {
	cursor: Cursor<Vec<u8>>,
}

impl BlobWriter {
	pub fn new() -> Self {
		Self {
			cursor: Cursor::new(Vec::new()),
		}
	}
	pub fn write_varint(&mut self, mut value: u64) -> Result<()> {
		while value >= 0x80 {
			self.cursor.write_all(&[((value as u8) & 0x7F) | 0x80])?;
			value >>= 7;
		}
		self.cursor.write_all(&[value as u8])?;
		Ok(())
	}
	pub fn write_u8(&mut self, value: u8) -> Result<()> {
		Ok(self.cursor.write_u8(value)?)
	}
	pub fn write_i32(&mut self, value: i32) -> Result<()> {
		Ok(self.cursor.write_i32::<LE>(value)?)
	}
	pub fn write_u64(&mut self, value: u64) -> Result<()> {
		Ok(self.cursor.write_u64::<LE>(value)?)
	}
	pub fn write_slice(&mut self, buf: &[u8]) -> Result<usize> {
		Ok(self.cursor.write(buf)?)
	}
	pub fn into_blob(self) -> Blob {
		Blob::from(self.cursor.into_inner())
	}
}
