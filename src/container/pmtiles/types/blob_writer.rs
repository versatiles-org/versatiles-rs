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

#[cfg(test)]
mod tests {
	use super::*;
	use byteorder::{LittleEndian, ReadBytesExt};
	use std::io::Cursor;

	#[test]
	fn test_write_varint() -> Result<()> {
		let mut writer = BlobWriter::new();
		writer.write_varint(300)?;
		assert_eq!(writer.into_blob().as_slice(), &[0xAC, 0x02]);
		Ok(())
	}

	#[test]
	fn test_write_u8() -> Result<()> {
		let mut writer = BlobWriter::new();
		writer.write_u8(0xFF)?;
		assert_eq!(writer.into_blob().as_slice(), &[0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_i32() -> Result<()> {
		let mut writer = BlobWriter::new();
		writer.write_i32(-1)?;
		let blob = writer.into_blob();
		let mut cursor = Cursor::new(blob.as_slice());
		assert_eq!(cursor.read_i32::<LittleEndian>()?, -1);
		Ok(())
	}

	#[test]
	fn test_write_u64() -> Result<()> {
		let mut writer = BlobWriter::new();
		writer.write_u64(u64::MAX)?;
		let blob = writer.into_blob();
		let mut cursor = Cursor::new(blob.as_slice());
		assert_eq!(cursor.read_u64::<LittleEndian>()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_write_slice() -> Result<()> {
		let mut writer = BlobWriter::new();
		let data = [0xDE, 0xAD, 0xBE, 0xEF];
		writer.write_slice(&data)?;
		assert_eq!(writer.into_blob().as_slice(), &data);
		Ok(())
	}
}
