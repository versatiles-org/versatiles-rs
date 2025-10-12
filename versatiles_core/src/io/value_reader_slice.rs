//! This module provides the `ValueReaderSlice` struct for reading values from a byte slice.
//!
//! # Overview
//!
//! The `ValueReaderSlice` struct allows for reading various data types from a byte slice using
//! either little-endian or big-endian byte order. It implements the `ValueReader` trait to provide
//! methods for reading integers, floating-point numbers, and other types of data from the slice. The
//! module also provides methods for managing the read position and creating sub-readers for reading
//! specific portions of the data.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::io::{ValueReader, ValueReaderSlice};
//! use anyhow::Result;
//!
//! fn main() -> Result<()> {
//!     let data = &[0x01, 0x02, 0x03, 0x04];
//!
//!     // Reading data with little-endian byte order
//!     let mut reader_le = ValueReaderSlice::new_le(data);
//!     assert_eq!(reader_le.read_u16()?, 0x0201);
//!
//!     // Reading data with big-endian byte order
//!     let mut reader_be = ValueReaderSlice::new_be(data);
//!     assert_eq!(reader_be.read_u16()?, 0x0102);
//!
//!     Ok(())
//! }
//! ```

#![allow(dead_code)]

use super::{SeekRead, ValueReader};
use anyhow::{Result, anyhow, bail};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{io::Cursor, marker::PhantomData};

/// A struct that provides reading capabilities from a byte slice using a specified byte order.
pub struct ValueReaderSlice<'a, E: ByteOrder> {
	pub _phantom: PhantomData<E>,
	pub cursor: Cursor<&'a [u8]>,
	pub len: u64,
}

impl<'a, E: ByteOrder> ValueReaderSlice<'a, E> {
	/// Creates a new `ValueReaderSlice` from a byte slice.
	///
	/// # Arguments
	///
	/// * `slice` - A reference to the byte slice to read.
	///
	/// # Returns
	///
	/// * A new `ValueReaderSlice` instance.
	#[must_use]
	pub fn new(slice: &'a [u8]) -> ValueReaderSlice<'a, E> {
		ValueReaderSlice {
			_phantom: PhantomData,
			len: slice.len() as u64,
			cursor: Cursor::new(slice),
		}
	}
}

impl<'a> ValueReaderSlice<'a, LittleEndian> {
	/// Creates a new `ValueReaderSlice` with little-endian byte order.
	///
	/// # Arguments
	///
	/// * `slice` - A reference to the byte slice to read.
	///
	/// # Returns
	///
	/// * A new `ValueReaderSlice` instance with little-endian byte order.
	#[must_use]
	pub fn new_le(slice: &'a [u8]) -> ValueReaderSlice<'a, LittleEndian> {
		ValueReaderSlice::new(slice)
	}
}

impl<'a> ValueReaderSlice<'a, BigEndian> {
	/// Creates a new `ValueReaderSlice` with big-endian byte order.
	///
	/// # Arguments
	///
	/// * `slice` - A reference to the byte slice to read.
	///
	/// # Returns
	///
	/// * A new `ValueReaderSlice` instance with big-endian byte order.
	#[must_use]
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
		let reader = ValueReaderSlice::new_le(&[0x80; 42]);
		assert_eq!(reader.len(), 42);
		Ok(())
	}

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
	fn test_read_i32_le() -> Result<()> {
		let blob = vec![0xFD, 0xFF, 0xFF, 0xFF]; // -1 in little-endian 32-bit
		let mut reader = ValueReaderSlice::new_le(&blob);

		assert_eq!(reader.read_i32()?, -3);
		Ok(())
	}

	#[test]
	fn test_read_i32_be() -> Result<()> {
		let blob = vec![0xFF, 0xFF, 0xFF, 0xFD]; // -1 in big-endian 32-bit
		let mut reader = ValueReaderSlice::new_be(&blob);

		assert_eq!(reader.read_i32()?, -3);
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

	#[test]
	fn test_get_sub_reader() -> Result<()> {
		let buf = vec![0x01, 0x02, 0x03, 0x04, 0x05];
		let mut reader = ValueReaderSlice::new_le(&buf);
		reader.set_position(1)?;
		let mut sub_reader = reader.get_sub_reader(3)?;
		assert_eq!(sub_reader.read_u8()?, 0x02);
		assert_eq!(sub_reader.read_u8()?, 0x03);
		assert_eq!(sub_reader.read_u8()?, 0x04);
		assert!(sub_reader.read_u8().is_err()); // Should be out of data
		Ok(())
	}

	#[test]
	fn test_sub_reader_out_of_bounds() -> Result<()> {
		let buf = vec![0x01, 0x02, 0x03];
		let mut reader = ValueReaderSlice::new_le(&buf);
		let result = reader.get_sub_reader(5);
		assert!(result.is_err());
		Ok(())
	}
}
