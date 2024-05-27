//! This module provides the `ValueWriterBlob` struct for writing values to an in-memory blob of data.
//!
//! # Overview
//!
//! The `ValueWriterBlob` struct allows for writing various data types to an in-memory blob using
//! either little-endian or big-endian byte order. It implements the `ValueWriter` trait to provide
//! methods for writing integers, floating-point numbers, strings, and custom binary formats. The
//! module also provides methods for converting the written data into a `Blob`.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::{ValueWriter, ValueWriterBlob}, types::Blob};
//! use anyhow::Result;
//!
//! fn main() -> Result<()> {
//!     let mut writer = ValueWriterBlob::new_le();
//!
//!     // Writing a varint
//!     writer.write_varint(1025)?;
//!     assert_eq!(writer.into_blob().into_vec(), vec![0b10000001,0b00001000]);
//!
//!     Ok(())
//! }
//! ```

#![allow(dead_code)]

use super::ValueWriter;
use crate::types::Blob;
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::io::{Cursor, Write};
use std::marker::PhantomData;

/// A struct that provides writing capabilities to an in-memory blob using a specified byte order.
pub struct ValueWriterBlob<E: ByteOrder> {
	_phantom: PhantomData<E>,
	cursor: Cursor<Vec<u8>>,
}

impl<E: ByteOrder> ValueWriterBlob<E> {
	/// Creates a new `ValueWriterBlob` instance.
	///
	/// # Returns
	///
	/// * A new `ValueWriterBlob` instance.
	pub fn new() -> ValueWriterBlob<E> {
		ValueWriterBlob {
			_phantom: PhantomData,
			cursor: Cursor::new(Vec::new()),
		}
	}

	/// Converts the written data into a `Blob`.
	///
	/// # Returns
	///
	/// * A `Blob` containing the written data.
	pub fn into_blob(self) -> Blob {
		Blob::from(self.cursor.into_inner())
	}
}

impl ValueWriterBlob<LittleEndian> {
	/// Creates a new `ValueWriterBlob` instance with little-endian byte order.
	///
	/// # Returns
	///
	/// * A new `ValueWriterBlob` instance with little-endian byte order.
	pub fn new_le() -> ValueWriterBlob<LittleEndian> {
		ValueWriterBlob::new()
	}
}

impl ValueWriterBlob<BigEndian> {
	/// Creates a new `ValueWriterBlob` instance with big-endian byte order.
	///
	/// # Returns
	///
	/// * A new `ValueWriterBlob` instance with big-endian byte order.
	pub fn new_be() -> ValueWriterBlob<BigEndian> {
		ValueWriterBlob::new()
	}
}

impl<E: ByteOrder> ValueWriter<E> for ValueWriterBlob<E> {
	fn get_writer(&mut self) -> &mut dyn Write {
		&mut self.cursor
	}

	fn position(&mut self) -> Result<u64> {
		Ok(self.cursor.position())
	}
}

impl<E: ByteOrder> Default for ValueWriterBlob<E> {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::ByteRange;

	#[test]
	fn test_write_varint() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_varint(300)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0b10101100, 0b00000010]);
		Ok(())
	}

	#[test]
	fn test_write_svarint() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_svarint(-75)?;
		assert_eq!(writer.into_blob().into_vec(), vec![149, 1]);
		Ok(())
	}

	#[test]
	fn test_write_u8() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_u8(255)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_i32() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_i32(-1)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_f32() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_f32(1.0)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0x00, 0x00, 0x80, 0x3F]);
		Ok(())
	}

	#[test]
	fn test_write_f64() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_f64(1.0)?;
		assert_eq!(
			writer.into_blob().into_vec(),
			vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F]
		);
		Ok(())
	}

	#[test]
	fn test_write_u32() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_u32(4294967295)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_u64() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_u64(18446744073709551615)?;
		assert_eq!(
			writer.into_blob().into_vec(),
			vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
		);
		Ok(())
	}

	#[test]
	fn test_write_blob() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		let blob = Blob::from(vec![0x01, 0x02, 0x03]);
		writer.write_blob(&blob)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0x01, 0x02, 0x03]);
		Ok(())
	}

	#[test]
	fn test_write_string() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_string("hello")?;
		assert_eq!(writer.into_blob().into_vec(), b"hello");
		Ok(())
	}

	#[test]
	fn test_write_range() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		let range = ByteRange {
			offset: 1,
			length: 2,
		};
		writer.write_range(&range)?;
		assert_eq!(
			writer.into_blob().into_vec(),
			vec![
				0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
				0x00, 0x00
			]
		);
		Ok(())
	}

	#[test]
	fn test_write_pbf_key() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_pbf_key(1, 0)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0x08]);
		Ok(())
	}

	#[test]
	fn test_write_pbf_packed_uint32() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_pbf_packed_uint32(&[100, 150, 300])?;
		assert_eq!(writer.into_blob().into_vec(), vec![5, 100, 150, 1, 172, 2]);
		Ok(())
	}

	#[test]
	fn test_write_pbf_string() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		writer.write_pbf_string("hello")?;
		assert_eq!(
			writer.into_blob().into_vec(),
			vec![0x05, b'h', b'e', b'l', b'l', b'o']
		);
		Ok(())
	}

	#[test]
	fn test_write_pbf_blob() -> Result<()> {
		let mut writer = ValueWriterBlob::<LittleEndian>::new();
		let blob = Blob::from(vec![0x01, 0x02, 0x03]);
		writer.write_pbf_blob(&blob)?;
		assert_eq!(writer.into_blob().into_vec(), vec![0x03, 0x01, 0x02, 0x03]);
		Ok(())
	}
}
