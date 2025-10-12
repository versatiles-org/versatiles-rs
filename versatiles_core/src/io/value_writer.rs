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
	fn get_writer(&mut self) -> &mut dyn Write;
	fn position(&mut self) -> Result<u64>;

	fn is_empty(&mut self) -> Result<bool> {
		Ok(self.position()? == 0)
	}

	fn write_varint(&mut self, mut value: u64) -> Result<()> {
		while value >= 0x80 {
			self.get_writer().write_all(&[((value as u8) & 0x7F) | 0x80])?;
			value >>= 7;
		}
		self.get_writer().write_all(&[value as u8])?;
		Ok(())
	}

	fn write_svarint(&mut self, value: i64) -> Result<()> {
		self.write_varint(((value << 1) ^ (value >> 63)) as u64)
	}

	fn write_u8(&mut self, value: u8) -> Result<()> {
		Ok(self.get_writer().write_u8(value)?)
	}

	fn write_i32(&mut self, value: i32) -> Result<()> {
		Ok(self.get_writer().write_i32::<E>(value)?)
	}

	fn write_f32(&mut self, value: f32) -> Result<()> {
		Ok(self.get_writer().write_f32::<E>(value)?)
	}

	fn write_f64(&mut self, value: f64) -> Result<()> {
		Ok(self.get_writer().write_f64::<E>(value)?)
	}

	fn write_u32(&mut self, value: u32) -> Result<()> {
		Ok(self.get_writer().write_u32::<E>(value)?)
	}

	fn write_u64(&mut self, value: u64) -> Result<()> {
		Ok(self.get_writer().write_u64::<E>(value)?)
	}

	fn write_blob(&mut self, blob: &Blob) -> Result<()> {
		self.get_writer().write_all(blob.as_slice())?;
		Ok(())
	}

	fn write_slice(&mut self, buf: &[u8]) -> Result<()> {
		self.get_writer().write_all(buf)?;
		Ok(())
	}

	fn write_string(&mut self, text: &str) -> Result<()> {
		self.get_writer().write_all(text.as_bytes())?;
		Ok(())
	}

	fn write_range(&mut self, range: &ByteRange) -> Result<()> {
		self.get_writer().write_u64::<E>(range.offset)?;
		self.get_writer().write_u64::<E>(range.length)?;
		Ok(())
	}

	fn write_pbf_key(&mut self, field_number: u32, wire_type: u8) -> Result<()> {
		self
			.write_varint((u64::from(field_number) << 3) | u64::from(wire_type))
			.context("Failed to write PBF key")
	}

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

	fn write_pbf_blob(&mut self, blob: &Blob) -> Result<()> {
		self
			.write_varint(blob.len())
			.context("Failed to write varint for blob length")?;
		self.write_blob(blob).context("Failed to write PBF blob")
	}

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
