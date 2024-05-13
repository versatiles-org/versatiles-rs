use crate::types::Blob;
use anyhow::{bail, Result};
use byteorder::{LittleEndian as LE, ReadBytesExt};
use std::io::Cursor;

pub struct BlobReader<'a> {
	cursor: Cursor<&'a [u8]>,
}

impl<'a> BlobReader<'a> {
	pub fn new(blob: &'a Blob) -> Self {
		Self {
			cursor: Cursor::new(blob.as_slice()),
		}
	}
	pub fn read_varint(&mut self) -> Result<u64> {
		let mut value = 0;
		let mut shift = 0;
		loop {
			let byte = self.cursor.read_u8()?;
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
	pub fn read_u8(&mut self) -> Result<u8> {
		Ok(self.cursor.read_u8()?)
	}
	pub fn read_i32(&mut self) -> Result<i32> {
		Ok(self.cursor.read_i32::<LE>()?)
	}
	pub fn read_u64(&mut self) -> Result<u64> {
		Ok(self.cursor.read_u64::<LE>()?)
	}
	pub fn set_position(&mut self, pos: u64) {
		self.cursor.set_position(pos);
	}
	#[allow(dead_code)]
	pub fn get_position(&mut self) -> u64 {
		self.cursor.position()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::Blob;

	#[test]
	fn test_read_varint() -> Result<()> {
		let data = vec![0b10101100, 0b00000010]; // Represents the varint for 300
		let blob = Blob::from(data);
		let mut reader = BlobReader::new(&blob);

		let varint = reader.read_varint()?;
		assert_eq!(varint, 300);
		Ok(())
	}

	#[test]
	fn test_read_varint_too_long() -> Result<()> {
		let data = vec![0x80; 10]; // More than 9 bytes with the MSB set to 1
		let blob = Blob::from(data);
		let mut reader = BlobReader::new(&blob);

		let result = reader.read_varint();
		assert!(result.is_err());
		Ok(())
	}

	#[test]
	fn test_read_u8() -> Result<()> {
		let data = vec![0x01, 0x02];
		let blob = Blob::from(data);
		let mut reader = BlobReader::new(&blob);

		assert_eq!(reader.read_u8()?, 0x01);
		assert_eq!(reader.read_u8()?, 0x02);
		Ok(())
	}

	#[test]
	fn test_read_i32() -> Result<()> {
		let data = vec![0xFF, 0xFF, 0xFF, 0xFF]; // -1 in little-endian 32-bit
		let blob = Blob::from(data);
		let mut reader = BlobReader::new(&blob);

		assert_eq!(reader.read_i32()?, -1);
		Ok(())
	}

	#[test]
	fn test_read_u64() -> Result<()> {
		let data = vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Max u64
		let blob = Blob::from(data);
		let mut reader = BlobReader::new(&blob);

		assert_eq!(reader.read_u64()?, u64::MAX);
		Ok(())
	}

	#[test]
	fn test_set_and_get_position() -> Result<()> {
		let data = vec![0x01, 0x02, 0x03, 0x04];
		let blob = Blob::from(data);
		let mut reader = BlobReader::new(&blob);

		reader.set_position(2);
		assert_eq!(reader.get_position(), 2);
		assert_eq!(reader.read_u8()?, 0x03);
		Ok(())
	}
}
