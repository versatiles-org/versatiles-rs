//! This module provides the `ValueWriterFile` struct for writing values to a file.
//!
//! # Overview
//!
//! The `ValueWriterFile` struct allows for writing various data types to a file using
//! either little-endian or big-endian byte order. It implements the `ValueWriter` trait to provide
//! methods for writing integers, floating-point numbers, strings, and custom binary formats.
//! The module also provides methods for managing the write position within the file.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::io::{ValueWriter, ValueWriterFile};
//! use anyhow::Result;
//! use std::fs::File;
//!
//! fn main() -> Result<()> {
//!     let path = std::env::temp_dir().join("temp2.txt");
//!     let file = File::create(&path)?;
//!     let mut writer = ValueWriterFile::new_le(file);
//!
//!     // Writing a string
//!     writer.write_string("hello")?;
//!
//!     Ok(())
//! }
//! ```

#![allow(dead_code)]

use super::ValueWriter;
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::marker::PhantomData;

/// A struct that provides writing capabilities to a file using a specified byte order.
pub struct ValueWriterFile<E: ByteOrder> {
	_phantom: PhantomData<E>,
	writer: BufWriter<File>,
}

impl<E: ByteOrder> ValueWriterFile<E> {
	/// Creates a new `ValueWriterFile` instance from a `File`.
	///
	/// # Arguments
	///
	/// * `file` - A `File` instance to write to.
	///
	/// # Returns
	///
	/// * A new `ValueWriterFile` instance.
	#[must_use]
	pub fn new(file: File) -> ValueWriterFile<E> {
		ValueWriterFile {
			_phantom: PhantomData,
			writer: BufWriter::new(file),
		}
	}
}

impl ValueWriterFile<LittleEndian> {
	/// Creates a new `ValueWriterFile` instance with little-endian byte order from a `File`.
	///
	/// # Arguments
	///
	/// * `file` - A `File` instance to write to.
	///
	/// # Returns
	///
	/// * A new `ValueWriterFile` instance with little-endian byte order.
	#[must_use]
	pub fn new_le(file: File) -> ValueWriterFile<LittleEndian> {
		ValueWriterFile::new(file)
	}
}

impl ValueWriterFile<BigEndian> {
	/// Creates a new `ValueWriterFile` instance with big-endian byte order from a `File`.
	///
	/// # Arguments
	///
	/// * `file` - A `File` instance to write to.
	///
	/// # Returns
	///
	/// * A new `ValueWriterFile` instance with big-endian byte order.
	#[must_use]
	pub fn new_be(file: File) -> ValueWriterFile<BigEndian> {
		ValueWriterFile::new(file)
	}
}

impl<E: ByteOrder> ValueWriter<E> for ValueWriterFile<E> {
	fn get_writer(&mut self) -> &mut dyn Write {
		&mut self.writer
	}

	fn position(&mut self) -> Result<u64> {
		Ok(self.writer.stream_position()?)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{Blob, ByteRange};
	use assert_fs::NamedTempFile;
	use std::fs::File;
	use std::io::{Read, SeekFrom};

	struct TempFile {
		f: NamedTempFile,
	}

	impl TempFile {
		pub fn new() -> Self {
			Self {
				f: NamedTempFile::new("temp.bin").unwrap(),
			}
		}

		pub fn file(&self) -> File {
			File::create(self.f.path()).unwrap()
		}

		pub fn content(&self) -> Vec<u8> {
			let mut content = Vec::new();
			let mut file = File::open(self.f.path()).unwrap();
			file.seek(SeekFrom::Start(0)).unwrap();
			file.read_to_end(&mut content).unwrap();
			content
		}
	}

	#[test]
	fn test_write_varint() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_varint(300)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0b10101100, 0b00000010]);
		Ok(())
	}

	#[test]
	fn test_write_svarint() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_svarint(-75)?;
		drop(writer);
		assert_eq!(temp.content(), vec![149, 1]);
		Ok(())
	}

	#[test]
	fn test_write_u8() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_u8(255)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_i32() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_i32(-1)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_f32() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_f32(1.0)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0x00, 0x00, 0x80, 0x3F]);
		Ok(())
	}

	#[test]
	fn test_write_f64() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_f64(1.0)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F]);
		Ok(())
	}

	#[test]
	fn test_write_u32() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_u32(4294967295)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_u64() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_u64(18446744073709551615)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
		Ok(())
	}

	#[test]
	fn test_write_blob() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		let blob = Blob::from(vec![0x01, 0x02, 0x03]);
		writer.write_blob(&blob)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0x01, 0x02, 0x03]);
		Ok(())
	}

	#[test]
	fn test_write_string() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_string("hello")?;
		drop(writer);
		assert_eq!(temp.content(), b"hello");
		Ok(())
	}

	#[test]
	fn test_write_range() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		let range = ByteRange { offset: 1, length: 2 };
		writer.write_range(&range)?;
		drop(writer);
		assert_eq!(
			temp.content(),
			vec![
				0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
			]
		);
		Ok(())
	}

	#[test]
	fn test_write_pbf_key() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_pbf_key(1, 0)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0x08]);
		Ok(())
	}

	#[test]
	fn test_write_pbf_packed_uint32() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_pbf_packed_uint32(&[100, 150, 300])?;
		drop(writer);
		assert_eq!(temp.content(), vec![5, 100, 150, 1, 172, 2]);
		Ok(())
	}

	#[test]
	fn test_write_pbf_string() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		writer.write_pbf_string("hello")?;
		drop(writer);
		assert_eq!(temp.content(), vec![0x05, b'h', b'e', b'l', b'l', b'o']);
		Ok(())
	}

	#[test]
	fn test_write_pbf_blob() -> Result<()> {
		let temp = TempFile::new();
		let mut writer = ValueWriterFile::new_le(temp.file());
		let blob = Blob::from(vec![0x01, 0x02, 0x03]);
		writer.write_pbf_blob(&blob)?;
		drop(writer);
		assert_eq!(temp.content(), vec![0x03, 0x01, 0x02, 0x03]);
		Ok(())
	}
}
