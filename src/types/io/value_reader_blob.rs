#![allow(dead_code)]

use super::{SeekRead, ValueReader, ValueReaderSlice};
use crate::types::Blob;
use anyhow::{anyhow, bail, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	io::{Cursor, Read, Seek, SeekFrom},
	marker::PhantomData,
};

pub struct ValueReaderBlob<E: ByteOrder> {
	_phantom: PhantomData<E>,
	cursor: Cursor<Vec<u8>>,
	len: u64,
}

impl<E: ByteOrder> ValueReaderBlob<E> {
	pub fn new(blob: Blob) -> ValueReaderBlob<E> {
		ValueReaderBlob {
			_phantom: PhantomData,
			len: blob.len(),
			cursor: Cursor::new(blob.into_vec()),
		}
	}
}

impl ValueReaderBlob<LittleEndian> {
	pub fn new_le(blob: Blob) -> ValueReaderBlob<LittleEndian> {
		ValueReaderBlob::new(blob)
	}
}

impl ValueReaderBlob<BigEndian> {
	pub fn new_be(blob: Blob) -> ValueReaderBlob<BigEndian> {
		ValueReaderBlob::new(blob)
	}
}

impl SeekRead for Cursor<Vec<u8>> {}

impl<'a, E: ByteOrder + 'a> ValueReader<'a, E> for ValueReaderBlob<E> {
	fn get_reader(&mut self) -> &mut dyn SeekRead {
		&mut self.cursor
	}

	fn len(&self) -> u64 {
		self.len
	}

	fn position(&mut self) -> u64 {
		self.cursor.position()
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		if position >= self.len {
			bail!("set position outside length")
		}
		self.cursor.set_position(position);
		Ok(())
	}

	fn get_sub_reader<'b>(&'b mut self, length: u64) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b,
	{
		let start = self.cursor.position();
		let end = start + length;
		if end > self.len {
			bail!("Requested sub-reader length exceeds remaining data");
		}

		self.cursor.set_position(end);

		Ok(Box::new(ValueReaderSlice {
			_phantom: PhantomData,
			len: length,
			cursor: Cursor::new(
				self
					.cursor
					.get_ref()
					.get(start as usize..end as usize)
					.ok_or(anyhow!("out of bounds"))?,
			),
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_len() -> Result<()> {
		let reader = ValueReaderBlob::new_le(Blob::from(vec![0x80; 42]));
		assert_eq!(reader.len(), 42);
		Ok(())
	}

	#[test]
	fn test_read_varint() -> Result<()> {
		let buf = vec![0b10101100, 0b00000010]; // Represents the varint for 300
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));

		let varint = reader.read_varint()?;
		assert_eq!(varint, 300);
		Ok(())
	}

	#[test]
	fn test_read_varint_too_long() -> Result<()> {
		let buf = vec![0x80; 10]; // More than 9 bytes with the MSB set to 1
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));

		let result = reader.read_varint();
		assert!(result.is_err());
		Ok(())
	}

	#[test]
	fn test_read_u8() -> Result<()> {
		let buf = vec![0x01, 0x02];
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));

		assert_eq!(reader.read_u8()?, 0x01);
		assert_eq!(reader.read_u8()?, 0x02);
		Ok(())
	}

	#[test]
	fn test_read_i32_le() -> Result<()> {
		let buf = vec![0xFD, 0xFF, 0xFF, 0xFF]; // -1 in little-endian 32-bit
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));

		assert_eq!(reader.read_i32()?, -3);
		Ok(())
	}

	#[test]
	fn test_read_i32_be() -> Result<()> {
		let buf = vec![0xFF, 0xFF, 0xFF, 0xFD]; // -1 in big-endian 32-bit
		let mut reader = ValueReaderBlob::new_be(Blob::from(buf));

		assert_eq!(reader.read_i32()?, -3);
		Ok(())
	}

	#[test]
	fn test_read_u64() -> Result<()> {
		let buf = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Max u64
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));

		assert_eq!(reader.read_u64()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_set_and_get_position() -> Result<()> {
		let buf = vec![0x01, 0x02, 0x03, 0x04];
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));

		reader.set_position(2)?;
		assert_eq!(reader.position(), 2);
		assert_eq!(reader.read_u8()?, 0x03);
		Ok(())
	}

	#[test]
	fn test_get_sub_reader() -> Result<()> {
		let buf = vec![0x01, 0x02, 0x03, 0x04, 0x05];
		let mut reader = ValueReaderBlob::new_le(Blob::from(buf));
		reader.set_position(1)?;

		let mut sub_reader = reader.get_sub_reader(3)?;
		assert_eq!(sub_reader.read_u8()?, 0x02);
		assert_eq!(sub_reader.read_u8()?, 0x03);
		assert_eq!(sub_reader.read_u8()?, 0x04);
		assert!(sub_reader.read_u8().is_err()); // Should be out of data
		Ok(())
	}
}