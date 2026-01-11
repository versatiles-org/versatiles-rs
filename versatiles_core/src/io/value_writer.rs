//! This module defines the `ValueWriter` trait for writing various types of values to different destinations.
//!
//! # Overview
//!
//! The `ValueWriter` trait provides an interface for writing data types such as integers, floating-point numbers,
//! strings, and custom binary formats (e.g., Protocol Buffers) to various destinations. Implementations of this trait
//! can handle writing data with little-endian or big-endian byte order and provide methods for managing the write
//! position and writing specific portions of the data.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{io::ValueWriter, Blob, ByteRange};
//! use anyhow::Result;
//! use byteorder::LittleEndian;
//! use std::io::Cursor;
//!
//! struct MockValueWriter {
//!     cursor: Cursor<Vec<u8>>,
//! }
//!
//! impl MockValueWriter {
//!     pub fn new() -> Self {
//!         Self {
//!             cursor: Cursor::new(Vec::new()),
//!         }
//!     }
//!
//!     pub fn into_inner(self) -> Vec<u8> {
//!         self.cursor.into_inner()
//!     }
//! }
//!
//! impl ValueWriter<LittleEndian> for MockValueWriter {
//!     fn get_writer(&mut self) -> &mut dyn std::io::Write {
//!         &mut self.cursor
//!     }
//!
//!     fn position(&mut self) -> Result<u64> {
//!         Ok(self.cursor.position())
//!     }
//! }
//!
//! fn main() -> Result<()> {
//!     let mut writer = MockValueWriter::new();
//!
//!     // Writing a varint
//!     writer.write_varint(300)?;
//!     assert_eq!(writer.into_inner(), vec![0b10101100, 0b00000010]);
//!
//!     Ok(())
//! }
//! ```

use super::ValueWriterBlob;
use crate::{Blob, ByteRange};
use anyhow::{Context, Result};
use byteorder::{ByteOrder, WriteBytesExt};
use std::io::Write;

/// A trait for writing values to various destinations with support for different byte orders.
#[allow(dead_code)]
pub trait ValueWriter<E: ByteOrder> {
	/// Returns a mutable reference to the underlying writer.
	///
	/// # Returns
	///
	/// A mutable reference to a type implementing [`std::io::Write`].
	fn get_writer(&mut self) -> &mut dyn Write;

	/// Returns the current write position.
	///
	/// # Returns
	///
	/// The current position as a `u64` offset from the start.
	///
	/// # Errors
	///
	/// Returns an error if the position cannot be determined.
	fn position(&mut self) -> Result<u64>;

	/// Returns `true` if the writer is empty (i.e., position is zero).
	///
	/// # Returns
	///
	/// `Ok(true)` if the writer is empty, `Ok(false)` otherwise.
	///
	/// # Errors
	///
	/// Returns an error if the position cannot be determined.
	fn is_empty(&mut self) -> Result<bool> {
		Ok(self.position()? == 0)
	}

	/// Writes an unsigned variable-length integer (varint) to the writer.
	///
	/// # Parameters
	///
	/// - `value`: The unsigned integer value to write as a varint.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_varint(&mut self, mut value: u64) -> Result<()> {
		while value >= 0x80 {
			#[allow(clippy::cast_possible_truncation)] // Safe: value & 0x7F always fits in u8
			self.get_writer().write_all(&[((value & 0x7F) as u8) | 0x80])?;
			value >>= 7;
		}
		#[allow(clippy::cast_possible_truncation)] // Safe: value always fits in u8 here
		self.get_writer().write_all(&[value as u8])?;
		Ok(())
	}

	/// Writes a signed variable-length integer (zigzag-encoded) to the writer.
	///
	/// # Parameters
	///
	/// - `value`: The signed integer value to write as a zigzag-encoded varint.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_svarint(&mut self, value: i64) -> Result<()> {
		self.write_varint(((value << 1) ^ (value >> 63)) as u64)
	}

	/// Writes an 8-bit unsigned integer to the writer.
	///
	/// # Parameters
	///
	/// - `value`: The `u8` value to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_u8(&mut self, value: u8) -> Result<()> {
		Ok(self.get_writer().write_u8(value)?)
	}

	/// Writes a 32-bit signed integer using the specified byte order.
	///
	/// # Parameters
	///
	/// - `value`: The `i32` value to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_i32(&mut self, value: i32) -> Result<()> {
		Ok(self.get_writer().write_i32::<E>(value)?)
	}

	/// Writes a 32-bit floating-point value using the specified byte order.
	///
	/// # Parameters
	///
	/// - `value`: The `f32` value to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_f32(&mut self, value: f32) -> Result<()> {
		Ok(self.get_writer().write_f32::<E>(value)?)
	}

	/// Writes a 64-bit floating-point value using the specified byte order.
	///
	/// # Parameters
	///
	/// - `value`: The `f64` value to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_f64(&mut self, value: f64) -> Result<()> {
		Ok(self.get_writer().write_f64::<E>(value)?)
	}

	/// Writes a 32-bit unsigned integer using the specified byte order.
	///
	/// # Parameters
	///
	/// - `value`: The `u32` value to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_u32(&mut self, value: u32) -> Result<()> {
		Ok(self.get_writer().write_u32::<E>(value)?)
	}

	/// Writes a 64-bit unsigned integer using the specified byte order.
	///
	/// # Parameters
	///
	/// - `value`: The `u64` value to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_u64(&mut self, value: u64) -> Result<()> {
		Ok(self.get_writer().write_u64::<E>(value)?)
	}

	/// Writes the contents of a [`Blob`] to the writer.
	///
	/// # Parameters
	///
	/// - `blob`: The [`Blob`] to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_blob(&mut self, blob: &Blob) -> Result<()> {
		self.get_writer().write_all(blob.as_slice())?;
		Ok(())
	}

	/// Writes a slice of bytes to the writer.
	///
	/// # Parameters
	///
	/// - `buf`: The byte slice to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_slice(&mut self, buf: &[u8]) -> Result<()> {
		self.get_writer().write_all(buf)?;
		Ok(())
	}

	/// Writes a UTF-8 string as bytes to the writer.
	///
	/// # Parameters
	///
	/// - `text`: The string to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_string(&mut self, text: &str) -> Result<()> {
		self.get_writer().write_all(text.as_bytes())?;
		Ok(())
	}

	/// Writes a [`ByteRange`] (offset and length) using the specified byte order.
	///
	/// # Parameters
	///
	/// - `range`: The [`ByteRange`] to write.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_range(&mut self, range: &ByteRange) -> Result<()> {
		self.get_writer().write_u64::<E>(range.offset)?;
		self.get_writer().write_u64::<E>(range.length)?;
		Ok(())
	}

	/// Writes a Protocol Buffers (PBF) field key (field number and wire type) as a varint.
	///
	/// # Parameters
	///
	/// - `field_number`: The PBF field number.
	/// - `wire_type`: The PBF wire type.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_pbf_key(&mut self, field_number: u32, wire_type: u8) -> Result<()> {
		self
			.write_varint((u64::from(field_number) << 3) | u64::from(wire_type))
			.context("Failed to write PBF key")
	}

	/// Writes a packed repeated field of unsigned 32-bit integers in Protocol Buffers format.
	///
	/// # Parameters
	///
	/// - `data`: The slice of `u32` values to write as packed varints.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_pbf_packed_uint32(&mut self, data: &[u32]) -> Result<()> {
		let mut writer = ValueWriterBlob::new_le();
		for &value in data {
			writer
				.write_varint(u64::from(value))
				.context("Failed to write varint for packed uint32")?;
		}
		self
			.write_pbf_blob(&writer.into_blob())
			.context("Failed to write packed uint32 blob")
	}

	/// Writes a Protocol Buffers (PBF) length-delimited blob.
	///
	/// # Parameters
	///
	/// - `blob`: The [`Blob`] to write, prefixed by its length as a varint.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_pbf_blob(&mut self, blob: &Blob) -> Result<()> {
		self
			.write_varint(blob.len())
			.context("Failed to write varint for blob length")?;
		self.write_blob(blob).context("Failed to write PBF blob")
	}

	/// Writes a Protocol Buffers (PBF) length-delimited UTF-8 string.
	///
	/// # Parameters
	///
	/// - `text`: The string to write, prefixed by its length as a varint.
	///
	/// # Errors
	///
	/// Returns an error if writing to the underlying writer fails.
	fn write_pbf_string(&mut self, text: &str) -> Result<()> {
		self
			.write_varint(text.len() as u64)
			.context("Failed to write varint for string length")?;
		self.write_string(text).context("Failed to write PBF string")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use byteorder::LittleEndian;
	use std::io::Cursor;

	pub struct MockValueWriter {
		cursor: Cursor<Vec<u8>>,
	}

	impl MockValueWriter {
		pub fn new() -> Self {
			Self {
				cursor: Cursor::new(Vec::new()),
			}
		}

		pub fn into_inner(self) -> Vec<u8> {
			self.cursor.into_inner()
		}
	}

	impl ValueWriter<LittleEndian> for MockValueWriter {
		fn get_writer(&mut self) -> &mut dyn Write {
			&mut self.cursor
		}

		fn position(&mut self) -> Result<u64> {
			Ok(self.cursor.position())
		}
	}

	#[test]
	fn test_write_varint() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_varint(300)?;
		assert_eq!(writer.into_inner(), vec![0b10101100, 0b00000010]);
		Ok(())
	}

	#[test]
	fn test_write_svarint() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_svarint(-75)?;
		assert_eq!(writer.into_inner(), vec![149, 1]);
		Ok(())
	}

	#[test]
	fn test_write_u8() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_u8(255)?;
		assert_eq!(writer.into_inner(), vec![0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_i32() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_i32(-1)?;
		assert_eq!(writer.into_inner(), vec![0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_f32() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_f32(1.0)?;
		assert_eq!(writer.into_inner(), vec![0x00, 0x00, 0x80, 0x3F]);
		Ok(())
	}

	#[test]
	fn test_write_f64() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_f64(1.0)?;
		assert_eq!(
			writer.into_inner(),
			vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F]
		);
		Ok(())
	}

	#[test]
	fn test_write_u32() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_u32(4294967295)?;
		assert_eq!(writer.into_inner(), vec![0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_u64() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_u64(18446744073709551615)?;
		assert_eq!(
			writer.into_inner(),
			vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
		);
		Ok(())
	}

	#[test]
	fn test_write_blob() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_blob(&Blob::from(vec![0x01, 0x02, 0x03]))?;
		assert_eq!(writer.into_inner(), vec![0x01, 0x02, 0x03]);
		Ok(())
	}

	#[test]
	fn test_write_string() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_string("hello")?;
		assert_eq!(writer.into_inner(), b"hello");
		Ok(())
	}

	#[test]
	fn test_write_range() -> Result<()> {
		let mut writer = MockValueWriter::new();
		let range = ByteRange { offset: 1, length: 2 };
		writer.write_range(&range)?;
		assert_eq!(
			writer.into_inner(),
			vec![
				0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
			]
		);
		Ok(())
	}

	#[test]
	fn test_write_pbf_key() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_pbf_key(1, 0)?;
		assert_eq!(writer.into_inner(), vec![0x08]);
		Ok(())
	}

	#[test]
	fn test_write_pbf_packed_uint32() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_pbf_packed_uint32(&[100, 150, 300])?;
		assert_eq!(writer.into_inner(), vec![5, 100, 150, 1, 172, 2]);
		Ok(())
	}

	#[test]
	fn test_write_pbf_string() -> Result<()> {
		let mut writer = MockValueWriter::new();
		writer.write_pbf_string("hello")?;
		assert_eq!(writer.into_inner(), vec![0x05, b'h', b'e', b'l', b'l', b'o']);
		Ok(())
	}

	#[test]
	fn test_write_pbf_blob() -> Result<()> {
		let mut writer = MockValueWriter::new();
		let blob = Blob::from(vec![0x01, 0x02, 0x03]);
		writer.write_pbf_blob(&blob)?;
		assert_eq!(writer.into_inner(), vec![0x03, 0x01, 0x02, 0x03]);
		Ok(())
	}
}
