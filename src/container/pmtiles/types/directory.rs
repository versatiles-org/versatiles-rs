use super::{EntriesSliceV3, EntriesV3, EntryV3};
use crate::types::Blob;
use anyhow::Result;
use std::fmt::Debug;

pub struct Directory {
	pub root_bytes: Blob,
	pub leaves_bytes: Blob,
}

impl Directory {
	pub fn new(entries: &EntriesV3, target_root_len: usize) -> Result<Self> {
		let entries = &entries.as_slice();

		if entries.len() < 16384 {
			let root_bytes = entries.serialize_entries()?;
			// Case1: the entire directory fits into the target len
			if root_bytes.len() <= target_root_len {
				return Ok(Directory {
					root_bytes,
					leaves_bytes: Blob::new_empty(),
				});
			}
		}

		// TODO: case 2: mixed tile entries/directory entries in root

		// case 3: root directory is leaf pointers only
		// use an iterative method, increasing the size of the leaf directory until the root fits

		let mut leaf_size: f32 = (entries.len() as f32 / 3500f32).max(4096f32);

		loop {
			let d = Self::build_roots_leaves(entries, leaf_size as usize)?;
			if d.root_bytes.len() <= target_root_len {
				return Ok(d);
			}
			leaf_size *= 1.2
		}
	}

	fn build_roots_leaves(entries: &EntriesSliceV3, leaf_size: usize) -> Result<Self> {
		let mut root_entries = EntriesV3::new();
		let mut leaves_bytes: Vec<u8> = Vec::new();

		let mut idx: usize = 0;
		while idx < entries.len() {
			let mut end = idx + leaf_size;
			if idx + leaf_size > entries.len() {
				end = entries.len()
			}
			let serialized = entries.slice(idx..end).serialize_entries()?;

			root_entries.push(EntryV3::new(
				entries.get(idx).tile_id,
				leaves_bytes.len() as u64,
				serialized.len() as u32,
				0,
			));
			leaves_bytes.copy_from_slice(serialized.as_slice());

			idx += leaf_size;
		}

		let root_bytes = root_entries.as_slice().serialize_entries()?;

		Ok(Directory {
			root_bytes,
			leaves_bytes: Blob::from(leaves_bytes),
		})
	}
}

impl Debug for Directory {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Directory")
			.field("root_bytes", &self.root_bytes.len())
			.field("leaves_bytes", &self.leaves_bytes.len())
			.finish()
	}
}
