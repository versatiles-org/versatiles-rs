use super::ByteRange;
use crate::shared::{Blob, Result, TileBBox, TileCoord3};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Cursor};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockDefinition {
	coord3: TileCoord3,
	bbox: TileBBox,
	tiles_range: ByteRange,
	index_range: ByteRange,
}

impl BlockDefinition {
	pub fn new(x: u64, y: u64, z: u8, bbox: TileBBox) -> Self {
		Self {
			coord3: TileCoord3::new(x, y, z),
			bbox,
			tiles_range: ByteRange::empty(),
			index_range: ByteRange::empty(),
		}
	}

	pub fn from_blob(buf: Blob) -> Result<Self> {
		let mut cursor = Cursor::new(buf.as_slice());

		let z = cursor.read_u8()?;
		let x = cursor.read_u32::<BE>()? as u64;
		let y = cursor.read_u32::<BE>()? as u64;

		let bbox = {
			let x_min = cursor.read_u8()? as u64;
			let y_min = cursor.read_u8()? as u64;
			let x_max = cursor.read_u8()? as u64;
			let y_max = cursor.read_u8()? as u64;
			TileBBox::new(x_min, y_min, x_max, y_max)
		};

		let offset = cursor.read_u64::<BE>()?;
		let tiles_length = cursor.read_u64::<BE>()?;
		let index_length = cursor.read_u32::<BE>()? as u64;

		let tiles_range = ByteRange::new(offset, tiles_length);
		let index_range = ByteRange::new(offset + tiles_length, index_length);

		Ok(Self {
			coord3: TileCoord3::new(x, y, z),
			bbox,
			tiles_range,
			index_range,
		})
	}

	pub fn with_tiles_range(mut self, range: ByteRange) -> Self {
		self.tiles_range = range;
		self
	}

	pub fn with_index_range(mut self, range: ByteRange) -> Self {
		self.index_range = range;
		self
	}

	pub fn count_tiles(&self) -> u64 {
		self.bbox.count_tiles()
	}

	pub fn as_blob(&self) -> Result<Blob> {
		let mut vec = Vec::with_capacity(33);
		vec.write_u8(self.coord3.z).unwrap();
		vec.write_u32::<BE>(self.coord3.x as u32)?;
		vec.write_u32::<BE>(self.coord3.y as u32)?;

		vec.write_u8(self.bbox.x_min as u8)?;
		vec.write_u8(self.bbox.y_min as u8)?;
		vec.write_u8(self.bbox.x_max as u8)?;
		vec.write_u8(self.bbox.y_max as u8)?;

		assert_eq!(
			self.tiles_range.offset + self.tiles_range.length,
			self.index_range.offset,
			"tiles_range and index_range do not match"
		);

		vec.write_u64::<BE>(self.tiles_range.offset)?;
		vec.write_u64::<BE>(self.tiles_range.length)?;
		vec.write_u32::<BE>(self.index_range.length as u32)?;

		Ok(Blob::from(vec))
	}

	pub fn get_sort_index(&self) -> u64 {
		let size = 2u64.pow(self.coord3.z as u32);
		let offset = (size * size - 1) / 3;
		offset + size * self.coord3.y + self.coord3.x
	}

	pub fn get_bbox(&self) -> &TileBBox {
		&self.bbox
	}

	pub fn get_tiles_range(&self) -> &ByteRange {
		&self.tiles_range
	}

	pub fn get_index_range(&self) -> &ByteRange {
		&self.index_range
	}

	pub fn get_x(&self) -> u64 {
		self.coord3.x
	}

	pub fn get_y(&self) -> u64 {
		self.coord3.y
	}

	pub fn get_z(&self) -> u8 {
		self.coord3.z
	}

	pub fn get_coord3(&self) -> TileCoord3 {
		self.coord3
	}

	#[cfg(test)]
	pub fn as_str(&self) -> String {
		let x_offset = self.coord3.x * 256;
		let y_offset = self.coord3.y * 256;
		format!(
			"[{},[{},{}],[{},{}]]",
			self.coord3.z,
			self.bbox.x_min + x_offset,
			self.bbox.y_min + y_offset,
			self.bbox.x_max + x_offset,
			self.bbox.y_max + y_offset
		)
	}
}

impl fmt::Debug for BlockDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("BlockDefinition")
			.field("x/y/z", &self.coord3)
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
	fn test_conversion() {
		let mut def1 = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		def1.tiles_range = ByteRange::new(4, 5);
		def1.index_range = ByteRange::new(9, 6);

		let def2 = BlockDefinition::from_blob(def1.as_blob().unwrap()).unwrap();

		assert_eq!(def1, def2);
	}

	#[test]
	fn test_count_tiles() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.count_tiles(), 16);
	}

	#[test]
	fn test_as_blob() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		let blob = def.as_blob().unwrap();
		assert_eq!(blob.len(), 33);
	}

	#[test]
	fn test_get_sort_index() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.get_sort_index(), 38);
	}

	#[test]
	fn test_as_str() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.as_str(), "[3,[256,512],[259,515]]");
	}

	#[test]
	fn test_debug() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		let debug_string = format!("{:?}", def);
		assert_eq!(
			debug_string,
			"BlockDefinition { x/y/z: TileCoord3(1, 2, 3), bbox: TileBBox [0,0,3,3] = 16, tiles_range: ByteRange[0,0], index_range: ByteRange[0,0] }"
		);
	}
}
