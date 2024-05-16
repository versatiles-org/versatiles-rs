use crate::types::{Blob, ByteRange};
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian, WriteBytesExt};
use std::{
	io::{Cursor, Write},
	marker::PhantomData,
};

pub struct BlobWriter<E: ByteOrder> {
	_phantom: PhantomData<E>,
	cursor: Cursor<Vec<u8>>,
}

impl<E: ByteOrder> BlobWriter<E> {
	fn new() -> BlobWriter<E> {
		BlobWriter {
			_phantom: PhantomData,
			cursor: Cursor::new(Vec::new()),
		}
	}

	pub fn len(&self) -> u64 {
		self.cursor.get_ref().len() as u64
	}

	#[allow(dead_code)]
	pub fn is_empty(&self) -> bool {
		self.cursor.get_ref().len() == 0
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
		Ok(self.cursor.write_i32::<E>(value)?)
	}

	pub fn write_u32(&mut self, value: u32) -> Result<()> {
		Ok(self.cursor.write_u32::<E>(value)?)
	}

	pub fn write_u64(&mut self, value: u64) -> Result<()> {
		Ok(self.cursor.write_u64::<E>(value)?)
	}

	pub fn write_blob(&mut self, blob: &Blob) -> Result<()> {
		self.cursor.write_all(blob.as_slice())?;
		Ok(())
	}

	pub fn write_slice(&mut self, buf: &[u8]) -> Result<()> {
		self.cursor.write_all(buf)?;
		Ok(())
	}

	pub fn write_range(&mut self, range: &ByteRange) -> Result<()> {
		self.cursor.write_u64::<E>(range.offset)?;
		self.cursor.write_u64::<E>(range.length)?;
		Ok(())
	}

	pub fn into_blob(self) -> Blob {
		Blob::from(self.cursor.into_inner())
	}
}

impl BlobWriter<LittleEndian> {
	pub fn new_le() -> BlobWriter<LittleEndian> {
		BlobWriter::new()
	}
}

impl BlobWriter<BigEndian> {
	pub fn new_be() -> BlobWriter<BigEndian> {
		BlobWriter::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use byteorder::{LittleEndian, ReadBytesExt};
	use std::io::Cursor;

	#[test]
	fn test_write_varint() -> Result<()> {
		let mut writer = BlobWriter::new_le();
		writer.write_varint(300)?;
		assert_eq!(writer.into_blob().as_slice(), &[0xAC, 0x02]);
		Ok(())
	}

	#[test]
	fn test_write_u8() -> Result<()> {
		let mut writer = BlobWriter::new_le();
		writer.write_u8(0xFF)?;
		assert_eq!(writer.into_blob().as_slice(), &[0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_i32() -> Result<()> {
		let mut writer = BlobWriter::new_le();
		writer.write_i32(-1)?;
		let blob = writer.into_blob();
		let mut cursor = Cursor::new(blob.as_slice());
		assert_eq!(cursor.read_i32::<LittleEndian>()?, -1);
		Ok(())
	}

	#[test]
	fn test_write_u64() -> Result<()> {
		let mut writer = BlobWriter::new_le();
		writer.write_u64(u64::MAX)?;
		let blob = writer.into_blob();
		let mut cursor = Cursor::new(blob.as_slice());
		assert_eq!(cursor.read_u64::<LittleEndian>()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_write_slice() -> Result<()> {
		let mut writer = BlobWriter::new_le();
		let data = [0xDE, 0xAD, 0xBE, 0xEF];
		writer.write_slice(&data)?;
		assert_eq!(writer.into_blob().as_slice(), &data);
		Ok(())
	}
}
