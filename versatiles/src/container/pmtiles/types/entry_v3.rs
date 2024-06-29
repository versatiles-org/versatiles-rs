use crate::types::ByteRange;
use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryV3 {
	pub tile_id: u64,
	pub range: ByteRange,
	pub run_length: u32,
}

impl EntryV3 {
	pub fn new(tile_id: u64, range: ByteRange, run_length: u32) -> Self {
		Self {
			tile_id,
			range,
			run_length,
		}
	}
}
