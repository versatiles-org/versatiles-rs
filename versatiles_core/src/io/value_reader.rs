//! This module defines the `ValueReader` trait for reading various types of values from different sources.
//!
//! # Overview
//!
//! The `ValueReader` trait provides an interface for reading data types such as integers, floating-point numbers,
//! strings, and custom binary formats (e.g., Protocol Buffers) from various sources. Implementations of this trait
//! can handle reading data with little-endian or big-endian byte order and provide methods for managing the read
//! position and creating sub-readers for reading specific portions of the data.
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

use crate::{Blob, ByteRange};
use anyhow::{Context, Result, bail};
use byteorder::{ByteOrder, ReadBytesExt};
use std::io::{Read, Seek};

/// A trait that extends both `Seek` and `Read`.
pub trait SeekRead: Seek + Read {}

#[allow(dead_code)]
/// A trait for reading values from various sources with support for different byte orders.
pub trait ValueReader<'a, E: ByteOrder + 'a> {
	fn get_reader(&mut self) -> &mut dyn SeekRead;

	fn len(&self) -> u64;
	fn position(&mut self) -> u64;
	fn set_position(&mut self, position: u64) -> Result<()>;

	fn is_empty(&self) -> bool {
		self.len() == 0
	}

	fn remaining(&mut self) -> u64 {
		self.len() - self.position()
	}

	fn has_remaining(&mut self) -> bool {
		self.remaining() > 0
	}

	fn read_varint(&mut self) -> Result<u64> {
		let mut value = 0;
		let mut shift = 0;
		loop {
			let byte = self.get_reader().read_u8()?;
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

	fn read_svarint(&mut self) -> Result<i64> {
		let sint_value = self.read_varint()? as i64;
		Ok((sint_value >> 1) ^ -(sint_value & 1))
	}

	fn read_f32(&mut self) -> Result<f32> {
		Ok(self.get_reader().read_f32::<E>()?)
	}

	fn read_f64(&mut self) -> Result<f64> {
		Ok(self.get_reader().read_f64::<E>()?)
	}

	fn read_u8(&mut self) -> Result<u8> {
		Ok(self.get_reader().read_u8()?)
	}

	fn read_i16(&mut self) -> Result<i16> {
		Ok(self.get_reader().read_i16::<E>()?)
	}

	fn read_i32(&mut self) -> Result<i32> {
		Ok(self.get_reader().read_i32::<E>()?)
	}

	fn read_i64(&mut self) -> Result<i64> {
		Ok(self.get_reader().read_i64::<E>()?)
	}

	fn read_u16(&mut self) -> Result<u16> {
		Ok(self.get_reader().read_u16::<E>()?)
	}

	fn read_u32(&mut self) -> Result<u32> {
		Ok(self.get_reader().read_u32::<E>()?)
	}

	fn read_u64(&mut self) -> Result<u64> {
		Ok(self.get_reader().read_u64::<E>()?)
	}

	fn read_blob(&mut self, length: u64) -> Result<Blob> {
		let mut blob = Blob::new_sized(length as usize);
		self.get_reader().read_exact(blob.as_mut_slice())?;
		Ok(blob)
	}

	fn read_string(&mut self, length: u64) -> Result<String> {
		let mut vec = vec![0u8; length as usize];
		self.get_reader().read_exact(&mut vec)?;
		Ok(String::from_utf8(vec)?)
	}

	fn read_range(&mut self) -> Result<ByteRange> {
		Ok(ByteRange::new(
			self.get_reader().read_u64::<E>()?,
			self.get_reader().read_u64::<E>()?,
		))
	}

	fn read_pbf_key(&mut self) -> Result<(u32, u8)> {
		let value = self.read_varint().context("Failed to read varint for PBF key")?;
		Ok(((value >> 3) as u32, (value & 0x07) as u8))
	}

	fn get_sub_reader<'b>(&'b mut self, length: u64) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b;

	fn get_pbf_sub_reader<'b>(&'b mut self) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b,
	{
		let length = self
			.read_varint()
			.context("Failed to read varint for sub-reader length")?;
		self.get_sub_reader(length).context("Failed to get sub-reader")
	}

	fn read_pbf_packed_uint32(&mut self) -> Result<Vec<u32>> {
		let mut reader = self
			.get_pbf_sub_reader()
			.context("Failed to get PBF sub-reader for packed uint32")?;
		let mut values = Vec::new();
		while reader.has_remaining() {
			values.push(
				reader
					.read_varint()
					.context("Failed to read varint for packed uint32")? as u32,
			);
		}
		drop(reader);
		Ok(values)
	}

	fn read_pbf_string(&mut self) -> Result<String> {
		let length = self.read_varint().context("Failed to read varint for string length")?;
		self.read_string(length).context("Failed to read PBF string")
	}

	fn read_pbf_blob(&mut self) -> Result<Blob> {
		let length = self.read_varint().context("Failed to read varint for blob length")?;
		self.read_blob(length).context("Failed to read PBF blob")
	}
}

#[cfg(test)]
mod tests {
	use super::super::ValueReaderSlice;
	use super::*;

	#[test]
	fn test_is_empty() {
		assert!(ValueReaderSlice::new_le(&[]).is_empty());
		assert!(!ValueReaderSlice::new_le(&[0]).is_empty());
	}

	#[test]
	fn test_read_varint() {
		let mut reader = ValueReaderSlice::new_le(&[0xAC, 0x02]);
		let value = reader.read_varint().unwrap();
		assert_eq!(value, 300);
	}

	#[test]
	fn test_read_svarint1() {
		let mut reader = ValueReaderSlice::new_le(&[0x96, 0x01]);
		assert_eq!(reader.read_svarint().unwrap(), 75);
	}

	#[test]
	fn test_read_svarint2() {
		let mut reader = ValueReaderSlice::new_le(&[0x95, 0x01]);
		assert_eq!(reader.read_svarint().unwrap(), -75);
	}

	#[test]
	fn test_read_f32_le() {
		let mut reader = ValueReaderSlice::new_le(&[0, 0, 0x80, 0x3F]); // 1.0 in f32
		assert_eq!(reader.read_f32().unwrap(), 1.0);
	}

	#[test]
	fn test_read_f32_be() {
		let mut reader = ValueReaderSlice::new_be(&[0x3F, 0x80, 0, 0]); // 1.0 in f32
		assert_eq!(reader.read_f32().unwrap(), 1.0);
	}

	#[test]
	fn test_read_f64_le() {
		let mut reader = ValueReaderSlice::new_le(&[0, 0, 0, 0, 0, 0, 0xF0, 0x3F]); // 1.0 in f64
		assert_eq!(reader.read_f64().unwrap(), 1.0);
	}

	#[test]
	fn test_read_f64_be() {
		let mut reader = ValueReaderSlice::new_be(&[0x3F, 0xF0, 0, 0, 0, 0, 0, 0]); // 1.0 in f64
		assert_eq!(reader.read_f64().unwrap(), 1.0);
	}

	#[test]
	fn test_read_u8() {
		let mut reader = ValueReaderSlice::new_le(&[0xFF]);
		assert_eq!(reader.read_u8().unwrap(), 255);
	}

	#[test]
	fn test_read_i32() {
		let mut reader = ValueReaderSlice::new_le(&[0xFF, 0xFF, 0xFF, 0xFF]); // -1 in i32
		assert_eq!(reader.read_i32().unwrap(), -1);
	}

	#[test]
	fn test_read_i64() {
		let mut reader = ValueReaderSlice::new_le(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // -1 in i64
		assert_eq!(reader.read_i64().unwrap(), -1);
	}

	#[test]
	fn test_read_u32() {
		let mut reader = ValueReaderSlice::new_le(&[0xFF, 0xFF, 0xFF, 0xFF]); // 4294967295 in u32
		assert_eq!(reader.read_u32().unwrap(), 4294967295);
	}

	#[test]
	fn test_read_u64() {
		let mut reader = ValueReaderSlice::new_le(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // 18446744073709551615 in u64
		assert_eq!(reader.read_u64().unwrap(), 18446744073709551615);
	}

	#[test]
	fn test_read_blob() {
		let data = vec![0x01, 0x02, 0x03, 0x04];
		let mut reader = ValueReaderSlice::new_le(&data);
		assert_eq!(reader.read_blob(3).unwrap().as_slice(), &data[0..3]);
	}

	#[test]
	fn test_read_string() {
		let mut reader = ValueReaderSlice::new_le(b"hello");
		assert_eq!(reader.read_string(5).unwrap(), "hello");
	}

	#[test]
	fn test_read_range() {
		let mut reader = ValueReaderSlice::new_be(&[0, 0, 0, 0, 0, 0, 0, 0x01, 0, 0, 0, 0, 0, 0, 0, 0x02]);
		let range = reader.read_range().unwrap();
		assert_eq!(range.offset, 1);
		assert_eq!(range.length, 2);
	}

	#[test]
	fn test_read_pbf_key() {
		let mut reader = ValueReaderSlice::new_le(&[0x08]);
		let (field_number, wire_type) = reader.read_pbf_key().unwrap();
		assert_eq!(field_number, 1);
		assert_eq!(wire_type, 0);
	}

	#[test]
	fn test_read_pbf_packed_uint32() {
		let mut reader = ValueReaderSlice::new_le(&[0x05, 0x64, 0x96, 0x01, 0xAC, 0x02]);
		assert_eq!(reader.read_pbf_packed_uint32().unwrap(), vec![100, 150, 300]);
	}

	#[test]
	fn test_read_pbf_string() {
		let mut reader = ValueReaderSlice::new_le(&[0x05, b'h', b'e', b'l', b'l', b'o']);
		assert_eq!(reader.read_pbf_string().unwrap(), "hello");
	}

	#[test]
	fn test_read_pbf_blob() {
		let mut reader = ValueReaderSlice::new_le(&[0x03, 0x01, 0x02, 0x03]);
		assert_eq!(reader.read_pbf_blob().unwrap().as_slice(), &[0x01, 0x02, 0x03]);
	}
}
