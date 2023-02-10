use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Read, ops::Range};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
	pub offset: u64,
	pub length: u64,
}
impl ByteRange {
	pub fn new(offset: u64, length: u64) -> ByteRange {
		ByteRange { offset, length }
	}
	pub fn empty() -> ByteRange {
		ByteRange { offset: 0, length: 0 }
	}
	pub fn from_reader(reader: &mut impl Read) -> ByteRange {
		ByteRange::new(reader.read_u64::<BE>().unwrap(), reader.read_u64::<BE>().unwrap())
	}
	pub fn write_to_buf(&self, writer: &mut impl WriteBytesExt) {
		writer.write_u64::<BE>(self.offset).unwrap();
		writer.write_u64::<BE>(self.length).unwrap();
	}
	pub fn as_range_usize(&self) -> Range<usize> {
		Range {
			start: (self.offset) as usize,
			end: (self.offset + self.length) as usize,
		}
	}
}

impl fmt::Debug for ByteRange {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("ByteRange[{},{}]", &self.offset, &self.length))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	#[test]
	fn conversion() {
		let range1 = ByteRange::new(23, 42);
		let mut cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
		range1.write_to_buf(&mut cursor);
		cursor.set_position(0);
		let range2 = ByteRange::from_reader(&mut cursor);
		assert_eq!(range1, range2);
	}
}
