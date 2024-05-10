#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryV3 {
	pub tile_id: u64,
	pub offset: u64,
	pub length: u32,
	pub run_length: u32,
}

impl EntryV3 {
	// Default constructor equivalent in Rust
	pub fn new() -> Self {
		Self {
			tile_id: 0,
			offset: 0,
			length: 0,
			run_length: 0,
		}
	}

	// Parameterized constructor
	pub fn with_values(tile_id: u64, offset: u64, length: u32, run_length: u32) -> Self {
		Self {
			tile_id,
			offset,
			length,
			run_length,
		}
	}
}

// Using Rust's binary search mechanism to find a tile
pub fn find_tile(entries: &[EntryV3], tile_id: u64) -> Option<EntryV3> {
	let result = entries.binary_search_by(|entry| entry.tile_id.cmp(&tile_id));

	match result {
		Ok(index) => Some(entries[index]),
		Err(0) => None, // If the search falls before the first element
		Err(index) => {
			// Adjust to last valid index if the index is out of bounds
			let last_valid_index = if index > 0 { index - 1 } else { 0 };
			if entries[last_valid_index].run_length == 0 {
				Some(entries[last_valid_index])
			} else if tile_id - entries[last_valid_index].tile_id < entries[last_valid_index].run_length as u64 {
				Some(entries[last_valid_index])
			} else {
				None
			}
		}
	}
}
