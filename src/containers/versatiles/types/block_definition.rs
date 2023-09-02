use super::ByteRange;
use crate::shared::{Result, TileBBox, TileCoord2, TileCoord3};
use byteorder::{BigEndian as BE, ReadBytesExt, WriteBytesExt};
use std::{fmt, io::Cursor, ops::Div};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockDefinition {
	offset: TileCoord3,       // block offset, for level 14 it's between [0,0] and [63,63]
	global_bbox: TileBBox,    // tile coverage, is usually [0,0,255,255]
	tiles_coverage: TileBBox, // tile coverage, is usually [0,0,255,255]
	tiles_range: ByteRange,
	index_range: ByteRange,
}

impl BlockDefinition {
	pub fn new(bbox: TileBBox) -> Self {
		let x = bbox.get_x_min().div(256u32);
		let y = bbox.get_y_min().div(256u32);
		let z = bbox.get_level();
		let global_bbox = bbox.clone();

		let tiles_coverage = TileBBox::new(
			z.min(8),
			bbox.get_x_min() - x * 256,
			bbox.get_y_min() - y * 256,
			bbox.get_x_max() - x * 256,
			bbox.get_y_max() - y * 256,
		);

		Self {
			offset: TileCoord3::new(x, y, z),
			global_bbox,
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

		let x_min = cursor.read_u8()? as u32;
		let y_min = cursor.read_u8()? as u32;
		let x_max = cursor.read_u8()? as u32;
		let y_max = cursor.read_u8()? as u32;

		let tiles_bbox = TileBBox::new(z.min(8), x_min, y_min, x_max, y_max);

		let offset = cursor.read_u64::<BE>()?;
		let tiles_length = cursor.read_u64::<BE>()?;
		let index_length = cursor.read_u32::<BE>()? as u64;

		let tiles_range = ByteRange::new(offset, tiles_length);
		let index_range = ByteRange::new(offset + tiles_length, index_length);

		let global_bbox = TileBBox::new(z, x_min + x * 256, y_min + y * 256, x_max + x * 256, y_max + y * 256);

		Ok(Self {
			offset: TileCoord3::new(x, y, z),
			global_bbox,
			tiles_coverage: tiles_bbox,
			tiles_range,
			index_range,
		})
	}

	pub fn set_tiles_range(&mut self, range: ByteRange) {
		self.tiles_range = range;
	}

	pub fn set_index_range(&mut self, range: ByteRange) {
		self.index_range = range;
	}

	pub fn count_tiles(&self) -> u64 {
		self.tiles_coverage.count_tiles()
	}

	pub fn as_vec(&self) -> Result<Vec<u8>> {
		let mut vec: Vec<u8> = Vec::with_capacity(33);
		vec.write_u8(self.offset.get_z())?;
		vec.write_u32::<BE>(self.offset.get_x())?;
		vec.write_u32::<BE>(self.offset.get_y())?;

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
		self.offset.get_sort_index()
	}

	/// global bbox of the defined tiles, e.g. [4096,4096,4351,4351]
	pub fn get_global_bbox(&self) -> &TileBBox {
		&self.global_bbox
	}

	pub fn get_tiles_range(&self) -> &ByteRange {
		&self.tiles_range
	}

	pub fn get_index_range(&self) -> &ByteRange {
		&self.index_range
	}

	#[allow(dead_code)]
	pub fn get_x(&self) -> u32 {
		self.offset.get_x()
	}

	#[allow(dead_code)]
	pub fn get_y(&self) -> u32 {
		self.offset.get_y()
	}

	pub fn get_z(&self) -> u8 {
		self.offset.get_z()
	}

	pub fn get_coord3(&self) -> &TileCoord3 {
		&self.offset
	}

	#[allow(dead_code)]
	pub fn get_coord_offset(&self) -> TileCoord2 {
		TileCoord2::new(self.offset.get_x() * 256, self.offset.get_y() * 256)
	}

	#[cfg(test)]
	pub fn as_str(&self) -> String {
		let x_offset = self.offset.get_x() * 256;
		let y_offset = self.offset.get_y() * 256;
		format!(
			"[{},[{},{}],[{},{}]]",
			self.offset.get_z(),
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
			.field("x/y/z", &self.offset)
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
	fn multitest() -> Result<()> {
		let mut def = BlockDefinition::new(TileBBox::new(12, 300, 400, 320, 450));
		def.tiles_range = ByteRange::new(4, 5);
		def.index_range = ByteRange::new(9, 6);

		assert_eq!(def, BlockDefinition::from_slice(&def.as_vec()?)?);
		assert_eq!(def.count_tiles(), 1071);
		assert_eq!(def.as_vec()?.len(), 33);
		assert_eq!(def.get_sort_index(), 5596502);
		assert_eq!(def.as_str(), "[12,[300,400],[320,450]]");
		assert_eq!(
			format!("{:?}", def),
			"BlockDefinition { x/y/z: TileCoord3(1, 1, 12), bbox: 8: [44,144,64,194] (1071), tiles_range: ByteRange[4,5], index_range: ByteRange[9,6] }"
		);

		Ok(())
	}
}
