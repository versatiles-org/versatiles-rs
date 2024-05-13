use crate::types::Blob;
use anyhow::{bail, Result};
use byteorder::{LittleEndian as LE, ReadBytesExt};
use std::io::Cursor;

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
