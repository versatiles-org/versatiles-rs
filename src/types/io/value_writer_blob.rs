#![allow(dead_code)]

use crate::types::Blob;

use super::ValueWriter;
use anyhow::Result;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use std::{
	io::{Cursor, Write},
	marker::PhantomData,
};

pub struct ValueWriterBlob<E: ByteOrder> {
	_phantom: PhantomData<E>,
	cursor: Cursor<Vec<u8>>,
}

impl<E: ByteOrder> ValueWriterBlob<E> {
	pub fn new() -> ValueWriterBlob<E> {
		ValueWriterBlob {
			_phantom: PhantomData,
			cursor: Cursor::new(Vec::new()),
		}
	}

	pub fn into_blob(self) -> Blob {
		Blob::from(self.cursor.into_inner())
	}
}

impl ValueWriterBlob<LittleEndian> {
	pub fn new_le() -> ValueWriterBlob<LittleEndian> {
		ValueWriterBlob::new()
	}
}

impl ValueWriterBlob<BigEndian> {
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
		let range = ByteRange { offset: 1, length: 2 };
		writer.write_range(&range)?;
		assert_eq!(
			writer.into_blob().into_vec(),
			vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
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
		assert_eq!(writer.into_blob().into_vec(), vec![0x05, b'h', b'e', b'l', b'l', b'o']);
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
