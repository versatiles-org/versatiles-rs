use crate::types::{Blob, ByteRange};
use anyhow::{anyhow, bail, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt};
use std::{
	io::{Cursor, Read},
	marker::PhantomData,
};

pub struct BlobReader<'a, E: ByteOrder> {
	_phantom: PhantomData<E>,
	cursor: Cursor<&'a [u8]>,
}

impl<'a, E: ByteOrder> BlobReader<'a, E> {
	fn new(blob: &'a Blob) -> BlobReader<'a, E> {
		BlobReader {
			_phantom: PhantomData,
			cursor: Cursor::new(blob.as_slice()),
		}
	}

	pub fn len(&self) -> u64 {
		self.cursor.get_ref().len() as u64
	}

	#[allow(dead_code)]
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn remaining(&self) -> u64 {
		self.cursor.get_ref().len() as u64 - self.cursor.position()
	}

	pub fn has_remaining(&self) -> bool {
		self.remaining() > 0
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

	pub fn read_f32(&mut self) -> Result<f32> {
		Ok(self.cursor.read_f32::<E>()?)
	}

	pub fn read_f64(&mut self) -> Result<f64> {
		Ok(self.cursor.read_f64::<E>()?)
	}

	pub fn read_u8(&mut self) -> Result<u8> {
		Ok(self.cursor.read_u8()?)
	}

	pub fn read_i32(&mut self) -> Result<i32> {
		Ok(self.cursor.read_i32::<E>()?)
	}

	#[allow(dead_code)]
	pub fn read_i64(&mut self) -> Result<i64> {
		Ok(self.cursor.read_i64::<E>()?)
	}

	pub fn read_u32(&mut self) -> Result<u32> {
		Ok(self.cursor.read_u32::<E>()?)
	}

	pub fn read_u64(&mut self) -> Result<u64> {
		Ok(self.cursor.read_u64::<E>()?)
	}

	//#[allow(dead_code)]
	//pub fn read_blob(&mut self, length: u64) -> Result<Blob> {
	//	let mut vec = vec![0u8; length as usize];
	//	self.cursor.read_exact(&mut vec)?;
	//	Ok(Blob::from(vec))
	//}

	#[allow(dead_code)]
	pub fn get_sub_reader(&mut self, length: u64) -> Result<BlobReader<E>> {
		let start = self.cursor.position() as usize;
		let end = start as u64 + length;
		self.cursor.set_position(end);
		Ok(BlobReader {
			_phantom: PhantomData,
			cursor: Cursor::new(
				self
					.cursor
					.get_ref()
					.get(start..end as usize)
					.ok_or(anyhow!("out of bounds"))?,
			),
		})
	}

	pub fn read_string(&mut self, length: u64) -> Result<String> {
		let mut vec = vec![0u8; length as usize];
		self.cursor.read_exact(&mut vec)?;
		Ok(String::from_utf8(vec)?)
	}

	pub fn read_range(&mut self) -> Result<ByteRange> {
		Ok(ByteRange::new(
			self.cursor.read_u64::<E>()?,
			self.cursor.read_u64::<E>()?,
		))
	}

	pub fn set_position(&mut self, pos: u64) {
		self.cursor.set_position(pos);
	}

	#[allow(dead_code)]
	pub fn position(&self) -> u64 {
		self.cursor.position()
	}
}

impl<'a> BlobReader<'a, LittleEndian> {
	pub fn new_le(blob: &'a Blob) -> BlobReader<'a, LittleEndian> {
		BlobReader::new(blob)
	}
}

impl<'a> BlobReader<'a, BigEndian> {
	pub fn new_be(blob: &'a Blob) -> BlobReader<'a, BigEndian> {
		BlobReader::new(blob)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::Blob;

	#[test]
	fn test_read_varint() -> Result<()> {
		let data = vec![0b10101100, 0b00000010]; // Represents the varint for 300
		let blob = Blob::from(data);
		let mut reader = BlobReader::new_le(&blob);

		let varint = reader.read_varint()?;
		assert_eq!(varint, 300);
		Ok(())
	}

	#[test]
	fn test_read_varint_too_long() -> Result<()> {
		let data = vec![0x80; 10]; // More than 9 bytes with the MSB set to 1
		let blob = Blob::from(data);
		let mut reader = BlobReader::new_le(&blob);

		let result = reader.read_varint();
		assert!(result.is_err());
		Ok(())
	}

	#[test]
	fn test_read_u8() -> Result<()> {
		let data = vec![0x01, 0x02];
		let blob = Blob::from(data);
		let mut reader = BlobReader::new_le(&blob);

		assert_eq!(reader.read_u8()?, 0x01);
		assert_eq!(reader.read_u8()?, 0x02);
		Ok(())
	}

	#[test]
	fn test_read_i32() -> Result<()> {
		let data = vec![0xFF, 0xFF, 0xFF, 0xFF]; // -1 in little-endian 32-bit
		let blob = Blob::from(data);
		let mut reader = BlobReader::new_le(&blob);

		assert_eq!(reader.read_i32()?, -1);
		Ok(())
	}

	#[test]
	fn test_read_u64() -> Result<()> {
		let data = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Max u64
		let blob = Blob::from(data);
		let mut reader = BlobReader::new_le(&blob);

		assert_eq!(reader.read_u64()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_set_and_get_position() -> Result<()> {
		let data = vec![0x01, 0x02, 0x03, 0x04];
		let blob = Blob::from(data);
		let mut reader = BlobReader::new_le(&blob);

		reader.set_position(2);
		assert_eq!(reader.position(), 2);
		assert_eq!(reader.read_u8()?, 0x03);
		Ok(())
	}
}
