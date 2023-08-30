use super::ByteRange;
use crate::shared::{Result, TileBBox, TileCoord2, TileCoord3};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Cursor};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockDefinition {
	coord3: TileCoord3,       // block offset, for level 14 it's between [0,0] and [63,63]
	tiles_coverage: TileBBox, // tile coverage, is usually [0,0,255,255]
	tiles_range: ByteRange,
	index_range: ByteRange,
}

impl BlockDefinition {
	pub fn new(x: u32, y: u32, z: u8, tiles_coverage: TileBBox) -> Self {
		if (tiles_coverage.get_x_max() > 255) || (tiles_coverage.get_y_max() > 255) {
			panic!("tiles_coverage bbox out of bounds ({tiles_coverage:?})")
		}
		Self {
			coord3: TileCoord3::new(x, y, z),
			tiles_coverage,
			tiles_range: ByteRange::empty(),
			index_range: ByteRange::empty(),
		}
	}

	pub fn from_slice(slice: &[u8]) -> Result<Self> {
		let mut cursor = Cursor::new(slice);

		let z = cursor.read_u8()?;
		let x = cursor.read_u32::<BE>()?;
		let y = cursor.read_u32::<BE>()?;

		let tiles_bbox = {
			let x_min = cursor.read_u8()? as u32;
			let y_min = cursor.read_u8()? as u32;
			let x_max = cursor.read_u8()? as u32;
			let y_max = cursor.read_u8()? as u32;
			TileBBox::new(z.min(8), x_min, y_min, x_max, y_max)
		};

		let offset = cursor.read_u64::<BE>()?;
		let tiles_length = cursor.read_u64::<BE>()?;
		let index_length = cursor.read_u32::<BE>()? as u64;

		let tiles_range = ByteRange::new(offset, tiles_length);
		let index_range = ByteRange::new(offset + tiles_length, index_length);

		Ok(Self {
			coord3: TileCoord3::new(x, y, z),
			tiles_coverage: tiles_bbox,
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
		self.tiles_coverage.count_tiles()
	}

	pub fn as_vec(&self) -> Result<Vec<u8>> {
		let mut vec: Vec<u8> = Vec::with_capacity(33);
		vec.write_u8(self.coord3.get_z())?;
		vec.write_u32::<BE>(self.coord3.get_x())?;
		vec.write_u32::<BE>(self.coord3.get_y())?;

		vec.write_u8(self.tiles_coverage.get_x_min() as u8)?;
		vec.write_u8(self.tiles_coverage.get_y_min() as u8)?;
		vec.write_u8(self.tiles_coverage.get_x_max() as u8)?;
		vec.write_u8(self.tiles_coverage.get_y_max() as u8)?;

		assert_eq!(
			self.tiles_range.offset + self.tiles_range.length,
			self.index_range.offset,
			"tiles_range and index_range do not match"
		);

		vec.write_u64::<BE>(self.tiles_range.offset)?;
		vec.write_u64::<BE>(self.tiles_range.length)?;
		vec.write_u32::<BE>(self.index_range.length as u32)?;

		Ok(vec)
	}

	pub fn get_sort_index(&self) -> u64 {
		self.coord3.get_sort_index()
	}

	/// global bbox of the defined tiles, e.g. [4096,4096,4351,4351]
	pub fn get_global_bbox(&self) -> TileBBox {
		self
			.tiles_coverage
			.clone()
			.shift_by(self.get_x() * 256, self.get_y() * 256)
	}

	pub fn get_tiles_range(&self) -> &ByteRange {
		&self.tiles_range
	}

	pub fn get_index_range(&self) -> &ByteRange {
		&self.index_range
	}

	pub fn get_x(&self) -> u32 {
		self.coord3.get_x()
	}

	pub fn get_y(&self) -> u32 {
		self.coord3.get_y()
	}

	pub fn get_z(&self) -> u8 {
		self.coord3.get_z()
	}

	pub fn get_coord3(&self) -> TileCoord3 {
		self.coord3
	}

	pub fn get_coord_offset(&self) -> TileCoord2 {
		TileCoord2::new(self.coord3.get_x() * 256, self.coord3.get_y() * 256)
	}

	#[cfg(test)]
	pub fn as_str(&self) -> String {
		let x_offset = self.coord3.get_x() * 256;
		let y_offset = self.coord3.get_y() * 256;
		format!(
			"[{},[{},{}],[{},{}]]",
			self.coord3.get_z(),
			self.tiles_coverage.get_x_min() + x_offset,
			self.tiles_coverage.get_y_min() + y_offset,
			self.tiles_coverage.get_x_max() + x_offset,
			self.tiles_coverage.get_y_max() + y_offset
		)
	}
}

impl fmt::Debug for BlockDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("BlockDefinition")
			.field("x/y/z", &self.coord3)
			.field("bbox", &self.tiles_coverage)
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
		let mut def1 = BlockDefinition::new(1, 2, 3, TileBBox::new_full(3));
		def1.tiles_range = ByteRange::new(4, 5);
		def1.index_range = ByteRange::new(9, 6);

		let def2 = BlockDefinition::from_slice(&def1.as_vec().unwrap()).unwrap();

		assert_eq!(def1, def2);
	}

	#[test]
	fn count_tiles() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		assert_eq!(def.count_tiles(), 16);
	}

	#[test]
	fn as_vec() {
		let def = BlockDefinition::new(1, 2, 3, TileBBox::new_full(2));
		let blob = def.as_vec().unwrap();
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
			"BlockDefinition { x/y/z: TileCoord3(1, 2, 3), bbox: 2: [0,0,3,3] (16), tiles_range: ByteRange[0,0], index_range: ByteRange[0,0] }"
		);
	}
}
