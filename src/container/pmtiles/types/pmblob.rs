use crate::types::Blob;
use anyhow::Result;
use byteorder::{LittleEndian as LE, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Write};

const SINGLE_BYTE_MAX: u8 = 250;
const U16_BYTE: u8 = 251;
const U32_BYTE: u8 = 252;
const U64_BYTE: u8 = 253;

const MAX_SINGLE_BYTE: u64 = SINGLE_BYTE_MAX as u64;
const MAX_U16: u64 = u16::MAX as u64;
const MAX_U32: u64 = u32::MAX as u64;

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
		let byte = self.cursor.read_u8()?;
		Ok(match byte {
			byte @ 0..=SINGLE_BYTE_MAX => byte as u64,
			U16_BYTE => self.cursor.read_u16::<LE>()? as u64,
			U32_BYTE => self.cursor.read_u32::<LE>()? as u64,
			U64_BYTE => self.cursor.read_u64::<LE>()?,
			_ => panic!("can't handle 128 bit integer"),
		})
	}
	pub fn read_u8(&mut self) -> Result<u8> {
		Ok(self.cursor.read_u8()?)
	}
	pub fn read_i32(&mut self) -> Result<i32> {
		Ok(self.cursor.read_i32::<LE>()?)
	}
	pub fn set_position(&mut self, pos: u64) {
		self.cursor.set_position(pos);
	}
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
	pub fn write_varint(&mut self, value: u64) -> Result<()> {
		if value <= MAX_SINGLE_BYTE {
			self.cursor.write_u8(value as u8)?
		} else if value <= MAX_U16 {
			self.cursor.write_u8(U16_BYTE)?;
			self.cursor.write_u16::<LE>(value as u16)?
		} else if value <= MAX_U32 {
			self.cursor.write_u8(U32_BYTE)?;
			self.cursor.write_u32::<LE>(value as u32)?
		} else {
			self.cursor.write_u8(U64_BYTE)?;
			self.cursor.write_u64::<LE>(value)?
		}
		Ok(())
	}
	pub fn write_u8(&mut self, value: u8) -> Result<()> {
		self.cursor.write_u8(value)?;
		Ok(())
	}
	pub fn write_i32(&mut self, value: i32) -> Result<()> {
		self.cursor.write_i32::<LE>(value)?;
		Ok(())
	}
	pub fn write_slice(&mut self, buf: &[u8]) -> Result<()> {
		self.cursor.write(buf)?;
		Ok(())
	}
	pub fn to_blob(self) -> Blob {
		Blob::from(self.cursor.into_inner())
	}
}
