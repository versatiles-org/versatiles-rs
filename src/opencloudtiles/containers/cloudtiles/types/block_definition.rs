use super::ByteRange;
use crate::opencloudtiles::types::TileBBox;
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

pub struct BlockDefinition {
	pub level: u64,
	pub x: u64,
	pub y: u64,
	pub bbox: TileBBox,
	pub tile_range: ByteRange,
}
impl BlockDefinition {
	pub fn new(level: u64, x: u64, y: u64, bbox: TileBBox) -> BlockDefinition {
		return BlockDefinition {
			level,
			x,
			y,
			bbox,
			tile_range: ByteRange::empty(),
		};
	}
	pub fn from_vec(buf: &[u8]) -> BlockDefinition {
		let mut cursor = Cursor::new(buf);
		let level = cursor.read_u8().unwrap() as u64;
		let x = cursor.read_u32::<BE>().unwrap() as u64;
		let y = cursor.read_u32::<BE>().unwrap() as u64;
		let x_min = cursor.read_u8().unwrap() as u64;
		let y_min = cursor.read_u8().unwrap() as u64;
		let x_max = cursor.read_u8().unwrap() as u64;
		let y_max = cursor.read_u8().unwrap() as u64;
		let bbox = TileBBox::new(x_min, y_min, x_max, y_max);
		let offset = cursor.read_u64::<BE>().unwrap();
		let length = cursor.read_u64::<BE>().unwrap();
		let tile_range = ByteRange::new(offset, length);
		return BlockDefinition {
			level,
			x,
			y,
			bbox,
			tile_range,
		};
	}
	pub fn count_tiles(&self) -> u64 {
		return self.bbox.count_tiles();
	}
	pub fn as_vec(&self) -> Vec<u8> {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		cursor.write_u8(self.level as u8).unwrap();
		cursor.write_u32::<BE>(self.x as u32).unwrap();
		cursor.write_u32::<BE>(self.y as u32).unwrap();
		cursor.write_u8(self.bbox.x_min as u8).unwrap();
		cursor.write_u8(self.bbox.y_min as u8).unwrap();
		cursor.write_u8(self.bbox.x_max as u8).unwrap();
		cursor.write_u8(self.bbox.y_max as u8).unwrap();
		cursor.write_u64::<BE>(self.tile_range.offset).unwrap();
		cursor.write_u64::<BE>(self.tile_range.length).unwrap();
		return cursor.into_inner();
	}
}
