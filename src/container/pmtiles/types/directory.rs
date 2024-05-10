use super::EntryV3;
use crate::{helper::compress_gzip, types::Blob};
use anyhow::Result;
use byteorder::{LittleEndian as LE, WriteBytesExt};
use std::io::Cursor;

pub struct Directory {
	pub root_bytes: Blob,
	pub leaves_bytes: Blob,
	pub num_leaves: u64,
}

impl Directory {
	pub fn new(entries: &[EntryV3], target_root_len: usize) -> Result<Self> {
		if entries.len() < 16384 {
			let root_bytes = serialize_entries(&entries)?;
			// Case1: the entire directory fits into the target len
			if root_bytes.len() <= target_root_len {
				return Ok(Directory {
					root_bytes,
					leaves_bytes: Blob::new_empty(),
					num_leaves: 0,
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

	fn build_roots_leaves(entries: &[EntryV3], leaf_size: usize) -> Result<Self> {
		let mut root_entries: Vec<EntryV3> = Vec::new();
		let mut leaves_bytes: Vec<u8> = Vec::new();
		let mut num_leaves: u64 = 0;

		let mut idx: usize = 0;
		while idx < entries.len() {
			num_leaves += 1;
			let mut end = idx + leaf_size;
			if idx + leaf_size > entries.len() {
				end = entries.len()
			}
			let serialized = serialize_entries(&entries[idx..end])?;

			root_entries.push(EntryV3::with_values(
				entries[idx].tile_id,
				leaves_bytes.len() as u64,
				serialized.len() as u32,
				0,
			));
			leaves_bytes.copy_from_slice(serialized.as_slice());

			idx += leaf_size;
		}

		let root_bytes = serialize_entries(&root_entries)?;
		return Ok(Directory {
			root_bytes,
			leaves_bytes: Blob::from(leaves_bytes),
			num_leaves,
		});
	}
}

fn serialize_entries(entries: &[EntryV3]) -> Result<Blob> {
	let mut blob = Cursor::new(vec![0u8; 0]);

	// Serialize the length of entries
	let len = entries.len() as u64;
	blob.write_u64::<LE>(len)?;

	// Serialize TileID deltas
	let mut last_id: u64 = 0;
	for entry in entries {
		let delta = entry.tile_id - last_id;
		blob.write_u64::<LE>(delta)?;
		last_id = entry.tile_id;
	}

	// Serialize RunLengths
	for entry in entries {
		blob.write_u64::<LE>(entry.run_length as u64)?;
	}

	// Serialize Lengths
	for entry in entries {
		blob.write_u64::<LE>(entry.length as u64)?;
	}

	// Serialize Offsets
	for i in 0..entries.len() {
		let offset = if i > 0 && entries[i].offset == entries[i - 1].offset + entries[i - 1].length as u64 {
			0
		} else {
			entries[i].offset + 1 // add 1 to not conflict with 0
		};
		blob.write_u64::<LE>(offset)?;
	}

	return Ok(compress_gzip(Blob::from(blob.into_inner()))?);
}
