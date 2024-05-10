use crate::container::ByteRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryV3 {
	pub tile_id: u64,
	pub range: ByteRange,
	pub run_length: u32,
}

impl EntryV3 {
	pub fn new(tile_id: u64, offset: u64, length: u32, run_length: u32) -> Self {
		Self {
			tile_id,
			range: ByteRange::new(offset, length as u64),
			run_length,
		}
	}
}
