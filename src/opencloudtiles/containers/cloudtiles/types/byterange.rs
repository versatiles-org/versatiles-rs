use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Read};

#[derive(Clone)]
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
	pub fn from_buf(reader: &mut impl Read) -> ByteRange {
		ByteRange::new(reader.read_u64::<BE>().unwrap(), reader.read_u64::<BE>().unwrap())
	}
	pub fn write_to_buf(&self, writer: &mut impl WriteBytesExt) {
		writer.write_u64::<BE>(self.offset).unwrap();
		writer.write_u64::<BE>(self.length).unwrap();
	}
}

impl fmt::Debug for ByteRange {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("ByteRange[{},{}]", &self.offset, &self.length))
	}
}
