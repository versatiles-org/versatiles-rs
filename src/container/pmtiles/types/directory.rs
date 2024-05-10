use super::EntryV3;
use crate::{
	helper::{compress_gzip, decompress_gzip},
	types::Blob,
};
use anyhow::Result;
use byteorder::{LittleEndian as LE, ReadBytesExt, WriteBytesExt};
use std::{cmp::Ordering, fmt::Debug, io::Cursor};

pub struct Directory {
	pub root_bytes: Blob,
	pub leaves_bytes: Blob,
}

impl Directory {
	pub fn new(entries: &[EntryV3], target_root_len: usize) -> Result<Self> {
		if entries.len() < 16384 {
			let root_bytes = serialize_entries(entries)?;
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

	fn build_roots_leaves(entries: &[EntryV3], leaf_size: usize) -> Result<Self> {
		let mut root_entries: Vec<EntryV3> = Vec::new();
		let mut leaves_bytes: Vec<u8> = Vec::new();

		let mut idx: usize = 0;
		while idx < entries.len() {
			let mut end = idx + leaf_size;
			if idx + leaf_size > entries.len() {
				end = entries.len()
			}
			let serialized = serialize_entries(&entries[idx..end])?;

			root_entries.push(EntryV3::new(
				entries[idx].tile_id,
				leaves_bytes.len() as u64,
				serialized.len() as u32,
				0,
			));
			leaves_bytes.copy_from_slice(serialized.as_slice());

			idx += leaf_size;
		}

		let root_bytes = serialize_entries(&root_entries)?;

		Ok(Directory {
			root_bytes,
			leaves_bytes: Blob::from(leaves_bytes),
		})
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

	compress_gzip(Blob::from(blob.into_inner()))
}

impl Debug for Directory {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Directory")
			.field("root_bytes", &self.root_bytes.len())
			.field("leaves_bytes", &self.leaves_bytes.len())
			.finish()
	}
}

pub struct EntriesV3 {
	entries: Vec<EntryV3>,
}

impl EntriesV3 {
	pub fn deserialize(data: &Blob) -> Result<Self> {
		let mut entries: Vec<EntryV3> = Vec::new();
		let data = decompress_gzip(data.clone())?;
		let mut reader = Cursor::new(data.as_slice());
		let num_entries = reader.read_u64::<LE>()? as usize;

		let mut last_id: u64 = 0;

		for _ in 0..num_entries {
			let diff = reader.read_u64::<LE>()?;
			last_id += diff;
			entries.push(EntryV3::new(last_id, 0, 0, 0));
		}

		for entry in entries.iter_mut() {
			entry.run_length = reader.read_u64::<LE>()? as u32;
		}

		for entry in entries.iter_mut() {
			entry.length = reader.read_u64::<LE>()? as u32;
		}

		for i in 0..num_entries {
			let tmp = reader.read_u64::<LE>()?;
			if i > 0 && tmp == 0 {
				entries[i].offset = entries[i - 1].offset + entries[i - 1].length as u64;
			} else {
				entries[i].offset = tmp - 1
			}
		}

		Ok(EntriesV3 { entries })
	}

	pub fn find_tile(&self, tile_id: u64) -> Option<EntryV3> {
		let mut m: i64 = 0;
		let mut n: i64 = self.entries.len() as i64 - 1;

		while m <= n {
			let k = (n + m) >> 1;
			let entry_id = self.entries[k as usize].tile_id;
			match tile_id.cmp(&entry_id) {
				Ordering::Greater => m = k + 1,
				Ordering::Less => n = k - 1,
				Ordering::Equal => return Some(self.entries[k as usize]),
			}
		}

		// at this point, m > n
		if n >= 0 {
			if self.entries[n as usize].run_length == 0 {
				return Some(self.entries[n as usize]);
			}
			if tile_id - self.entries[n as usize].tile_id < self.entries[n as usize].run_length as u64 {
				return Some(self.entries[n as usize]);
			}
		}

		None
	}
}
