use super::{tile_compression::PMTilesCompression, tile_type::PMTilesType};
use crate::{
	container::{ByteRange, TilesReaderParameters},
	types::Blob,
};
use anyhow::{ensure, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

#[derive(Debug, PartialEq)]
pub struct HeaderV3 {
	pub root_dir: ByteRange,
	pub metadata: ByteRange,
	pub leaf_dirs: ByteRange,
	pub tile_data: ByteRange,
	pub addressed_tiles_count: u64,
	pub tile_entries_count: u64,
	pub tile_contents_count: u64,
	pub clustered: bool,
	pub internal_compression: PMTilesCompression,
	pub tile_compression: PMTilesCompression,
	pub tile_type: PMTilesType,
	pub min_zoom: u8,
	pub max_zoom: u8,
	pub min_lon_e7: i32,
	pub min_lat_e7: i32,
	pub max_lon_e7: i32,
	pub max_lat_e7: i32,
	pub center_zoom: u8,
	pub center_lon_e7: i32,
	pub center_lat_e7: i32,
}

impl HeaderV3 {
	pub fn try_from(parameters: &TilesReaderParameters) -> Result<Self> {
		use PMTilesCompression as PC;
		use PMTilesType as PT;

		let bbox_pyramid = &parameters.bbox_pyramid;
		let bbox = bbox_pyramid.get_geo_bbox();

		Ok(Self {
			root_dir: ByteRange::new(0, 0),
			metadata: ByteRange::new(0, 0),
			leaf_dirs: ByteRange::new(0, 0),
			tile_data: ByteRange::new(0, 0),
			addressed_tiles_count: 0,
			tile_entries_count: 0,
			tile_contents_count: 0,
			clustered: false,
			internal_compression: PC::Gzip,
			tile_compression: PC::from_value(parameters.tile_compression).unwrap_or(PC::Unknown),
			tile_type: PT::from_value(parameters.tile_format).unwrap_or(PT::UNKNOWN),
			min_zoom: bbox_pyramid.get_zoom_min().unwrap_or(0),
			max_zoom: bbox_pyramid.get_zoom_max().unwrap_or(14),
			min_lon_e7: (bbox[0] * 1e7) as i32,
			min_lat_e7: (bbox[1] * 1e7) as i32,
			max_lon_e7: (bbox[2] * 1e7) as i32,
			max_lat_e7: (bbox[3] * 1e7) as i32,
			center_zoom: 0,
			center_lon_e7: ((bbox[2] - bbox[0]) * 5e6) as i32,
			center_lat_e7: ((bbox[3] - bbox[1]) * 5e6) as i32,
		})
	}

	pub fn serialize(&self) -> Blob {
		let mut buffer = Vec::new();
		buffer.extend_from_slice(b"PMTiles");
		buffer.push(3); // Version

		// Serialize fields to little-endian
		buffer.write_u64::<LittleEndian>(self.root_dir.offset).unwrap();
		buffer.write_u64::<LittleEndian>(self.root_dir.length).unwrap();
		buffer.write_u64::<LittleEndian>(self.metadata.offset).unwrap();
		buffer.write_u64::<LittleEndian>(self.metadata.length).unwrap();
		buffer.write_u64::<LittleEndian>(self.leaf_dirs.offset).unwrap();
		buffer.write_u64::<LittleEndian>(self.leaf_dirs.length).unwrap();
		buffer.write_u64::<LittleEndian>(self.tile_data.offset).unwrap();
		buffer.write_u64::<LittleEndian>(self.tile_data.length).unwrap();
		buffer.write_u64::<LittleEndian>(self.addressed_tiles_count).unwrap();
		buffer.write_u64::<LittleEndian>(self.tile_entries_count).unwrap();
		buffer.write_u64::<LittleEndian>(self.tile_contents_count).unwrap();

		// Serialize the boolean `clustered` as a byte
		let clustered_val = if self.clustered { 1u8 } else { 0u8 };
		buffer.push(clustered_val);

		// Continue with the rest of the fields
		buffer.push(self.internal_compression as u8);
		buffer.push(self.tile_compression as u8);
		buffer.push(self.tile_type as u8);
		buffer.push(self.min_zoom);
		buffer.push(self.max_zoom);
		buffer.write_i32::<LittleEndian>(self.min_lon_e7).unwrap();
		buffer.write_i32::<LittleEndian>(self.min_lat_e7).unwrap();
		buffer.write_i32::<LittleEndian>(self.max_lon_e7).unwrap();
		buffer.write_i32::<LittleEndian>(self.max_lat_e7).unwrap();
		buffer.push(self.center_zoom);
		buffer.write_i32::<LittleEndian>(self.center_lon_e7).unwrap();
		buffer.write_i32::<LittleEndian>(self.center_lat_e7).unwrap();

		Blob::from(buffer)
	}

	pub fn deserialize(blob: &Blob) -> Result<Self> {
		let buffer = blob.as_slice();

		ensure!(buffer.len() == 127, "pmtiles magic number exception");
		ensure!(&buffer[0..7] == b"PMTiles", "pmtiles magic number exception");
		ensure!(buffer[7] == 3, "pmtiles version: must be 3");

		let mut cursor = Cursor::new(buffer);
		cursor.set_position(8); // Skip PMTiles and version byte

		let header = Self {
			root_dir: ByteRange::new(cursor.read_u64::<LittleEndian>()?, cursor.read_u64::<LittleEndian>()?),
			metadata: ByteRange::new(cursor.read_u64::<LittleEndian>()?, cursor.read_u64::<LittleEndian>()?),
			leaf_dirs: ByteRange::new(cursor.read_u64::<LittleEndian>()?, cursor.read_u64::<LittleEndian>()?),
			tile_data: ByteRange::new(cursor.read_u64::<LittleEndian>()?, cursor.read_u64::<LittleEndian>()?),
			addressed_tiles_count: cursor.read_u64::<LittleEndian>()?,
			tile_entries_count: cursor.read_u64::<LittleEndian>()?,
			tile_contents_count: cursor.read_u64::<LittleEndian>()?,
			clustered: cursor.read_u8()? == 1,
			internal_compression: PMTilesCompression::from_u8(cursor.read_u8()?)?,
			tile_compression: PMTilesCompression::from_u8(cursor.read_u8()?)?,
			tile_type: PMTilesType::from_u8(cursor.read_u8()?)?,
			min_zoom: cursor.read_u8()?,
			max_zoom: cursor.read_u8()?,
			min_lon_e7: cursor.read_i32::<LittleEndian>()?,
			min_lat_e7: cursor.read_i32::<LittleEndian>()?,
			max_lon_e7: cursor.read_i32::<LittleEndian>()?,
			max_lat_e7: cursor.read_i32::<LittleEndian>()?,
			center_zoom: cursor.read_u8()?,
			center_lon_e7: cursor.read_i32::<LittleEndian>()?,
			center_lat_e7: cursor.read_i32::<LittleEndian>()?,
		};

		Ok(header)
	}

	pub fn len() -> usize {
		127
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn header_serialization_deserialization() {
		let header = HeaderV3 {
			root_dir: ByteRange::new(123456789, 987654321),
			metadata: ByteRange::new(111111111, 222222222),
			leaf_dirs: ByteRange::new(333333333, 444444444),
			tile_data: ByteRange::new(555555555, 666666666),
			addressed_tiles_count: 777777777,
			tile_entries_count: 888888888,
			tile_contents_count: 999999999,
			clustered: true,
			internal_compression: PMTilesCompression::None,
			tile_compression: PMTilesCompression::Gzip,
			tile_type: PMTilesType::JPEG,
			min_zoom: 4,
			max_zoom: 5,
			min_lon_e7: 6000000,
			min_lat_e7: 7000000,
			max_lon_e7: 8000000,
			max_lat_e7: 9000000,
			center_zoom: 10,
			center_lon_e7: 11000000,
			center_lat_e7: 12000000,
		};

		let serialized_data = header.serialize();
		let deserialized_header = HeaderV3::deserialize(&serialized_data).unwrap();

		assert_eq!(header, deserialized_header);
	}
}
