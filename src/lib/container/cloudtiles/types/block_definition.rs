use super::ByteRange;
use crate::helper::*;
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Cursor, ops::Div};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockDefinition {
	pub x: u64,
	pub y: u64,
	pub z: u64,
	pub bbox: TileBBox,
	pub tile_range: ByteRange,
}
impl BlockDefinition {
	pub fn new(x: u64, y: u64, z: u64, bbox: TileBBox) -> BlockDefinition {
		BlockDefinition {
			x,
			y,
			z,
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
		let offset = cursor.read_u64::<BE>().unwrap();
		let length = cursor.read_u64::<BE>().unwrap();

		let bbox = TileBBox::new(x_min, y_min, x_max, y_max);
		let tile_range = ByteRange::new(offset, length);

		BlockDefinition {
			z: level,
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

		cursor.write_u8(self.z as u8).unwrap();
		cursor.write_u32::<BE>(self.x as u32).unwrap();
		cursor.write_u32::<BE>(self.y as u32).unwrap();
		cursor.write_u8(self.bbox.x_min as u8).unwrap();
		cursor.write_u8(self.bbox.y_min as u8).unwrap();
		cursor.write_u8(self.bbox.x_max as u8).unwrap();
		cursor.write_u8(self.bbox.y_max as u8).unwrap();
		cursor.write_u64::<BE>(self.tile_range.offset).unwrap();
		cursor.write_u64::<BE>(self.tile_range.length).unwrap();

		Blob::from_vec(cursor.into_inner())
	}
	#[allow(dead_code)]
	pub fn as_str(&self) -> String {
		let x_offset = self.x * 256;
		let y_offset = self.y * 256;
		format!(
			"[{},[{},{}],[{},{}]]",
			self.z,
			self.bbox.x_min + x_offset,
			self.bbox.y_min + y_offset,
			self.bbox.x_max + x_offset,
			self.bbox.y_max + y_offset
		)
	}
	pub fn get_sort_index(&self) -> u64 {
		let size = 2u64.pow(self.z as u32);
		let offset = (size * size - 1).div(3);
		offset + size * self.y + self.x
	}
}

impl fmt::Debug for BlockDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("BlockDefinition")
			.field("x/y/z", &TileCoord3::new(self.x, self.y, self.z))
			.field("bbox", &self.bbox)
			.field("tile_range", &self.tile_range)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn conversion() {
		let mut def1 = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		def1.tile_range = ByteRange::new(4, 5);
		let def2 = BlockDefinition::from_blob(def1.as_blob());
		assert_eq!(def1, def2);
	}
}
