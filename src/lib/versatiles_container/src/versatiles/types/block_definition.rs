use super::ByteRange;
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Cursor, ops::Div};
use versatiles_shared::{Blob, TileBBox, TileCoord3};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockDefinition {
	pub x: u64,
	pub y: u64,
	pub z: u8,
	pub bbox: TileBBox,
	pub tiles_range: ByteRange,
	pub index_range: ByteRange,
}
impl BlockDefinition {
	pub fn new(x: u64, y: u64, z: u8, bbox: TileBBox) -> BlockDefinition {
		BlockDefinition {
			x,
			y,
			z,
			bbox,
			tiles_range: ByteRange::empty(),
			index_range: ByteRange::empty(),
		}
	}
	pub fn from_blob(buf: Blob) -> BlockDefinition {
		let mut cursor = Cursor::new(buf.as_slice());

		let z = cursor.read_u8().unwrap();
		let x = cursor.read_u32::<BE>().unwrap() as u64;
		let y = cursor.read_u32::<BE>().unwrap() as u64;

		let x_min = cursor.read_u8().unwrap() as u64;
		let y_min = cursor.read_u8().unwrap() as u64;
		let x_max = cursor.read_u8().unwrap() as u64;
		let y_max = cursor.read_u8().unwrap() as u64;
		let bbox = TileBBox::new(x_min, y_min, x_max, y_max);

		let offset = cursor.read_u64::<BE>().unwrap();
		let tiles_length = cursor.read_u64::<BE>().unwrap();
		let index_length = cursor.read_u32::<BE>().unwrap() as u64;

		let tiles_range = ByteRange::new(offset, tiles_length);
		let index_range = ByteRange::new(offset + tiles_length, index_length);

		BlockDefinition {
			z,
			x,
			y,
			bbox,
			tiles_range,
			index_range,
		}
	}
	pub fn count_tiles(&self) -> u64 {
		self.bbox.count_tiles()
	}
	pub fn as_blob(&self) -> Blob {
		let vec = Vec::new();
		let mut cursor = Cursor::new(vec);

		cursor.write_u8(self.z).unwrap();
		cursor.write_u32::<BE>(self.x as u32).unwrap();
		cursor.write_u32::<BE>(self.y as u32).unwrap();

		cursor.write_u8(self.bbox.x_min as u8).unwrap();
		cursor.write_u8(self.bbox.y_min as u8).unwrap();
		cursor.write_u8(self.bbox.x_max as u8).unwrap();
		cursor.write_u8(self.bbox.y_max as u8).unwrap();

		assert!(
			self.tiles_range.offset + self.tiles_range.length == self.index_range.offset,
			"{} + {} == {}",
			self.tiles_range.offset,
			self.tiles_range.length,
			self.index_range.offset
		);
		cursor.write_u64::<BE>(self.tiles_range.offset).unwrap();
		cursor.write_u64::<BE>(self.tiles_range.length).unwrap();
		cursor.write_u32::<BE>(self.index_range.length as u32).unwrap();

		Blob::from(cursor.into_inner())
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
			.field("tiles_range", &self.tiles_range)
			.field("index_range", &self.index_range)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn conversion() {
		let mut def1 = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		def1.tiles_range = ByteRange::new(4, 5);
		def1.index_range = ByteRange::new(9, 6);

		let def2 = BlockDefinition::from_blob(def1.as_blob());

		assert_eq!(def1, def2);
	}
	#[test]
	fn count_tiles() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.count_tiles(), 16);
	}

	#[test]
	fn as_blob() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		let blob = def.as_blob();
		assert_eq!(blob.len(), 33);
	}

	#[test]
	fn get_sort_index() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.get_sort_index(), 38);
	}

	#[test]
	fn as_str() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.as_str(), "[3,[256,512],[259,515]]");
	}

	#[test]
	fn debug() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		let debug_string = format!("{:?}", def);
		assert_eq!(
		debug_string,
		"BlockDefinition { x/y/z: TileCoord3(1, 2, 3), bbox: TileBBox [0,0,3,3] = 16, tiles_range: ByteRange[0,0], index_range: ByteRange[0,0] }"
	);
	}
}
