use crate::types::Blob;
use anyhow::{bail, Result};
use byteorder::{LittleEndian as LE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Write};

pub struct BlobReader<'a> {
	cursor: Cursor<&'a [u8]>,
}

impl<'a> BlobReader<'a> {
	pub fn new(blob: &'a Blob) -> Self {
		Self {
			cursor: Cursor::new(blob.as_slice()),
		}
	}
	pub fn read_varint(&mut self) -> Result<u64> {
		let mut value = 0;
		let mut shift = 0;
		loop {
			let byte = self.cursor.read_u8()?;
			value |= ((byte as u64) & 0x7F) << shift;
			if byte & 0x80 == 0 {
				break;
			}
			shift += 7;
			if shift >= 70 {
				bail!("Varint too long");
			}
		}
		Ok(value)
	}
	pub fn read_u8(&mut self) -> Result<u8> {
		Ok(self.cursor.read_u8()?)
	}
	pub fn read_i32(&mut self) -> Result<i32> {
		Ok(self.cursor.read_i32::<LE>()?)
	}
	pub fn read_u64(&mut self) -> Result<u64> {
		Ok(self.cursor.read_u64::<LE>()?)
	}
	pub fn set_position(&mut self, pos: u64) {
		self.cursor.set_position(pos);
	}
	#[allow(dead_code)]
	pub fn get_position(&mut self) -> u64 {
		self.cursor.position()
	}
}
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
