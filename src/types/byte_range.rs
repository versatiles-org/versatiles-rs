use anyhow::Result;
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
#[cfg(test)]
use std::io::Cursor;
use std::ops::Range;
use std::{fmt, io::Read};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ByteRange {
	pub offset: u64,
	pub length: u64,
}

impl ByteRange {
	pub fn new(offset: u64, length: u64) -> Self {
		Self { offset, length }
	}

	pub fn empty() -> Self {
		Self { offset: 0, length: 0 }
	}

	#[cfg(test)]
	pub fn from_buf(buf: &[u8]) -> Result<Self> {
		use anyhow::ensure;

		ensure!(buf.len() == 16, "byte range buffer must be 16 bytes long");
		let mut cursor = Cursor::new(buf);
		let offset = cursor.read_u64::<BE>()?;
		let length = cursor.read_u64::<BE>()?;
		Ok(Self::new(offset, length))
	}

	pub fn from_reader(reader: &mut impl Read) -> Result<Self> {
		Ok(Self::new(reader.read_u64::<BE>()?, reader.read_u64::<BE>()?))
	}

	pub fn write_to_buf(&self, writer: &mut impl WriteBytesExt) -> Result<()> {
		writer.write_u64::<BE>(self.offset)?;
		writer.write_u64::<BE>(self.length)?;
		Ok(())
	}

	pub fn shift_forward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset + offset,
			length: self.length,
		}
	}

	pub fn shift_backward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset - offset,
			length: self.length,
		}
	}

	pub fn as_range_usize(&self) -> Range<usize> {
		Range {
			start: self.offset as usize,
			end: (self.offset + self.length) as usize,
		}
	}
}

impl fmt::Debug for ByteRange {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "ByteRange[{},{}]", self.offset, self.length)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn conversion() {
		let range1 = ByteRange::new(23, 42);
		let mut cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
		range1.write_to_buf(&mut cursor).unwrap();
		cursor.set_position(0);
		let range2 = ByteRange::from_reader(&mut cursor).unwrap();
		assert_eq!(range1, range2);
	}

	#[test]
	fn new() {
		let range = ByteRange::new(23, 42);
		assert_eq!(range.offset, 23);
		assert_eq!(range.length, 42);
	}

	#[test]
	fn empty() {
		let range = ByteRange::empty();
		assert_eq!(range.offset, 0);
		assert_eq!(range.length, 0);
	}

	#[test]
	fn write_to_buf() {
		let range = ByteRange::new(23, 42);
		let mut buf: Vec<u8> = Vec::new();
		range.write_to_buf(&mut buf).unwrap();
		assert_eq!(buf.len(), 16); // 2 u64 values take up 16 bytes
		let mut cursor = Cursor::new(buf);
		let offset = cursor.read_u64::<BE>().unwrap();
		let length = cursor.read_u64::<BE>().unwrap();
		assert_eq!(offset, 23);
		assert_eq!(length, 42);
	}

	#[test]
	fn as_range_usize() {
		let range = ByteRange::new(23, 42);
		let range_usize = range.as_range_usize();
		assert_eq!(range_usize.start, 23);
		assert_eq!(range_usize.end, 65); // 23 + 42 = 65
	}

	#[test]
	fn debug() {
		let range = ByteRange::new(23, 42);
		assert_eq!(format!("{:?}", range), "ByteRange[23,42]");
	}
}
