#![allow(dead_code)]

use super::ValueWriter;
use crate::types::Blob;
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	fs::File,
	io::{BufWriter, Seek, Write},
	marker::PhantomData,
};

pub struct ValueWriterFile<E: ByteOrder> {
	_phantom: PhantomData<E>,
	writer: BufWriter<File>,
}

impl<E: ByteOrder> ValueWriterFile<E> {
	pub fn new(file: File) -> ValueWriterFile<E> {
		ValueWriterFile {
			_phantom: PhantomData,
			writer: BufWriter::new(file),
		}
	}
}

impl ValueWriterFile<LittleEndian> {
	pub fn new_le(file: File) -> ValueWriterFile<LittleEndian> {
		ValueWriterFile::new(file)
	}
}

impl ValueWriterFile<BigEndian> {
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
	use assert_fs::NamedTempFile;

	use crate::types::ByteRange;

	use super::*;
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
			vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
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
