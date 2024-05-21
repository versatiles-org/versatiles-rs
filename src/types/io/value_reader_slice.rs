use super::{SeekRead, ValueReader};
use crate::types::Blob;
use anyhow::{anyhow, bail, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	io::{Cursor, Read, Seek},
	marker::PhantomData,
};

pub struct ValueReaderSlice<'a, E: ByteOrder> {
	pub _phantom: PhantomData<E>,
	pub cursor: Cursor<&'a [u8]>,
	pub len: u64,
}

impl<'a, E: ByteOrder> ValueReaderSlice<'a, E> {
	pub fn new(slice: &'a [u8]) -> ValueReaderSlice<'a, E> {
		ValueReaderSlice {
			_phantom: PhantomData,
			len: slice.len() as u64,
			cursor: Cursor::new(slice),
		}
	}
}

impl<'a> ValueReaderSlice<'a, LittleEndian> {
	pub fn new_le(slice: &'a [u8]) -> ValueReaderSlice<'a, LittleEndian> {
		ValueReaderSlice::new(slice)
	}
}

impl<'a> ValueReaderSlice<'a, BigEndian> {
	pub fn new_be(slice: &'a [u8]) -> ValueReaderSlice<'a, BigEndian> {
		ValueReaderSlice::new(slice)
	}
}

impl SeekRead for Cursor<&[u8]> {}

impl<'a, E: ByteOrder + 'a> ValueReader<'a, E> for ValueReaderSlice<'a, E> {
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
	fn test_read_varint() -> Result<()> {
		let blob = vec![0b10101100, 0b00000010]; // Represents the varint for 300
		let mut reader = ValueReaderSlice::new_le(&blob);

		let varint = reader.read_varint()?;
		assert_eq!(varint, 300);
		Ok(())
	}

	#[test]
	fn test_read_varint_too_long() -> Result<()> {
		let blob = vec![0x80; 10]; // More than 9 bytes with the MSB set to 1
		let mut reader = ValueReaderSlice::new_le(&blob);

		let result = reader.read_varint();
		assert!(result.is_err());
		Ok(())
	}

	#[test]
	fn test_read_u8() -> Result<()> {
		let blob = vec![0x01, 0x02];
		let mut reader = ValueReaderSlice::new_le(&blob);

		assert_eq!(reader.read_u8()?, 0x01);
		assert_eq!(reader.read_u8()?, 0x02);
		Ok(())
	}

	#[test]
	fn test_read_i32() -> Result<()> {
		let blob = vec![0xFF, 0xFF, 0xFF, 0xFF]; // -1 in little-endian 32-bit
		let mut reader = ValueReaderSlice::new_le(&blob);

		assert_eq!(reader.read_i32()?, -1);
		Ok(())
	}

	#[test]
	fn test_read_u64() -> Result<()> {
		let blob = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Max u64
		let mut reader = ValueReaderSlice::new_le(&blob);

		assert_eq!(reader.read_u64()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_set_and_get_position() -> Result<()> {
		let blob = vec![0x01, 0x02, 0x03, 0x04];
		let mut reader = ValueReaderSlice::new_le(&blob);

		reader.set_position(2)?;
		assert_eq!(reader.position(), 2);
		assert_eq!(reader.read_u8()?, 0x03);
		Ok(())
	}
}
