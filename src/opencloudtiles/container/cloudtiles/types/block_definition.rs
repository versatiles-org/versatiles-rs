use super::ByteRange;
use crate::opencloudtiles::lib::{Blob, TileBBox, TileCoord3};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Cursor};

pub struct BlockDefinition {
	pub level: u64,
	pub x: u64,
	pub y: u64,
	pub bbox: TileBBox,
	pub tile_range: ByteRange,
}
impl BlockDefinition {
	pub fn new(level: u64, x: u64, y: u64, bbox: TileBBox) -> BlockDefinition {
		BlockDefinition {
			level,
			x,
			y,
			bbox,
			tile_range: ByteRange::empty(),
		}
	}
	pub fn from_blob(buf: Blob) -> BlockDefinition {
		let mut cursor = Cursor::new(buf.as_slice());
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

		BlockDefinition {
			level,
			x,
			y,
			bbox,
			tile_range,
		}
	}
	pub fn count_tiles(&self) -> u64 {
		self.bbox.count_tiles()
	}
	pub fn as_blob(&self) -> Blob {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);
		cursor.write_u8(self.level as u8).unwrap();
		cursor.write_u32::<BE>(self.x as u32).unwrap();
		cursor.write_u32::<BE>(self.y as u32).unwrap();
		cursor.write_u8(self.bbox.get_x_min() as u8).unwrap();
		cursor.write_u8(self.bbox.get_y_min() as u8).unwrap();
		cursor.write_u8(self.bbox.get_x_max() as u8).unwrap();
		cursor.write_u8(self.bbox.get_y_max() as u8).unwrap();
		cursor.write_u64::<BE>(self.tile_range.offset).unwrap();
		cursor.write_u64::<BE>(self.tile_range.length).unwrap();

		Blob::from_vec(cursor.into_inner())
	}
}

impl fmt::Debug for BlockDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("BlockDefinition")
			.field("z/y/x", &TileCoord3::new(self.level, self.y, self.x))
			.field("bbox", &self.bbox)
			.field("tile_range", &self.tile_range)
			.finish()
	}
}
