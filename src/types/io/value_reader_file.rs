#![allow(dead_code)]

use super::{SeekRead, ValueReader, ValueReaderBlob};
use crate::types::Blob;
use anyhow::{anyhow, bail, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	fs::File,
	io::{BufReader, Cursor, Read, Seek, SeekFrom, Write},
	marker::PhantomData,
	sync::Arc,
};

// Define the ValueReaderFile struct
pub struct ValueReaderFile<E: ByteOrder> {
	_phantom: PhantomData<E>,
	reader: BufReader<File>,
	len: u64,
}

impl<E: ByteOrder> ValueReaderFile<E> {
	pub fn new(file: File) -> Result<ValueReaderFile<E>> {
		let len = file.metadata()?.len();
		Ok(ValueReaderFile {
			_phantom: PhantomData,
			reader: BufReader::new(file),
			len,
		})
	}
}

impl SeekRead for BufReader<File> {}

impl ValueReaderFile<LittleEndian> {
	pub fn new_le(file: File) -> Result<ValueReaderFile<LittleEndian>> {
		ValueReaderFile::new(file)
	}
}

impl ValueReaderFile<BigEndian> {
	pub fn new_be(file: File) -> Result<ValueReaderFile<BigEndian>> {
		ValueReaderFile::new(file)
	}
}

impl<'a, E: ByteOrder + 'a> ValueReader<'a, E> for ValueReaderFile<E> {
	fn get_reader(&mut self) -> &mut dyn SeekRead {
		&mut self.reader
	}

	fn len(&self) -> u64 {
		self.len
	}

	fn position(&mut self) -> u64 {
		self.reader.stream_position().unwrap()
	}

	fn set_position(&mut self, position: u64) -> Result<()> {
		if position >= self.len {
			bail!("set position outside length")
		}
		self.reader.seek(SeekFrom::Start(position))?;
		Ok(())
	}

	fn get_sub_reader<'b>(&'b mut self, length: u64) -> Result<Box<dyn ValueReader<'b, E> + 'b>>
	where
		E: 'b,
	{
		let start = self.reader.stream_position()?;
		let end = start + length;
		if end > self.len {
			bail!("sub-reader length exceeds file length");
		}

		let mut buffer = vec![0; length as usize];
		self.reader.read_exact(&mut buffer)?;

		Ok(Box::new(ValueReaderBlob::<E>::new(Blob::from(buffer))))
	}
}

#[cfg(test)]
mod tests {
	use assert_fs::{fixture::FileWriteBin, NamedTempFile};

	use super::*;

	fn create_temp_file_with_content(content: &[u8]) -> Result<File> {
		let file = NamedTempFile::new("test.bin")?;
		file.write_binary(content)?;
		Ok(File::open(file)?)
	}

	#[test]
	fn test_len() -> Result<()> {
		let file = create_temp_file_with_content(&vec![0x80; 42])?;
		let reader = ValueReaderFile::new_le(file)?;
		assert_eq!(reader.len(), 42);
		Ok(())
	}

	#[test]
	fn test_read_varint() -> Result<()> {
		let file = create_temp_file_with_content(&[172, 2])?; // Represents the varint for 300
		let mut reader = ValueReaderFile::new_le(file)?;
		assert_eq!(reader.read_varint()?, 300);
		Ok(())
	}

	#[test]
	fn test_read_varint_too_long() -> Result<()> {
		let content = vec![0x80; 10]; // More than 9 bytes with the MSB set to 1
		let file = create_temp_file_with_content(&content)?;
		let mut reader = ValueReaderFile::new_le(file)?;
		assert!(reader.read_varint().is_err());
		Ok(())
	}

	#[test]
	fn test_read_u8() -> Result<()> {
		let file = create_temp_file_with_content(&[0x01, 0x02])?;
		let mut reader = ValueReaderFile::new_le(file)?;
		assert_eq!(reader.read_u8()?, 0x01);
		assert_eq!(reader.read_u8()?, 0x02);
		Ok(())
	}

	#[test]
	fn test_read_i32_le() -> Result<()> {
		let content = vec![0xFD, 0xFF, 0xFF, 0xFF]; // -1 in little-endian 32-bit
		let file = create_temp_file_with_content(&content)?;
		let mut reader = ValueReaderFile::new_le(file)?;
		assert_eq!(reader.read_i32()?, -3);
		Ok(())
	}

	#[test]
	fn test_read_i32_be() -> Result<()> {
		let content = vec![0xFF, 0xFF, 0xFF, 0xFD]; // -1 in big-endian 32-bit
		let file = create_temp_file_with_content(&content)?;
		let mut reader = ValueReaderFile::new_be(file)?;
		assert_eq!(reader.read_i32()?, -3);
		Ok(())
	}

	#[test]
	fn test_read_u64() -> Result<()> {
		let content = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Max u64
		let file = create_temp_file_with_content(&content)?;
		let mut reader = ValueReaderFile::new_le(file)?;
		assert_eq!(reader.read_u64()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_set_and_get_position() -> Result<()> {
		let file = create_temp_file_with_content(&[0x01, 0x02, 0x03, 0x04])?;
		let mut reader = ValueReaderFile::new_le(file)?;
		reader.set_position(2)?;
		assert_eq!(reader.position(), 2);
		assert_eq!(reader.read_u8()?, 0x03);
		Ok(())
	}

	#[test]
	fn test_get_sub_reader() -> Result<()> {
		let file = create_temp_file_with_content(&[0x01, 0x02, 0x03, 0x04, 0x05])?;
		let mut reader = ValueReaderFile::new_le(file)?;
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
		let file = create_temp_file_with_content(&[0x01, 0x02, 0x03])?;
		let mut reader = ValueReaderFile::new_le(file)?;
		let result = reader.get_sub_reader(5);
		assert!(result.is_err());
		Ok(())
	}
}
