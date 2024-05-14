use super::{BlobReader, BlobWriter, Directory, EntryV3};
use crate::types::Blob;
use anyhow::Result;
use std::{cmp::Ordering, io::Write, slice::SliceIndex};

#[derive(Debug, PartialEq)]
pub struct EntriesV3 {
	entries: Vec<EntryV3>,
}

impl EntriesV3 {
	pub fn new() -> Self {
		Self { entries: Vec::new() }
	}

	pub fn from_blob(data: &Blob) -> Result<Self> {
		let mut entries: Vec<EntryV3> = Vec::new();
		let mut reader = BlobReader::new(data);

		let num_entries = reader.read_varint()? as usize;

		if num_entries > 10_000_000_000 {
			panic!("there is something wrong: PMTiles with more then 10 billion tiles?")
		}

		let mut last_id: u64 = 0;

		for _ in 0..num_entries {
			let diff = reader.read_varint()?;
			last_id += diff;
			entries.push(EntryV3::new(last_id, 0, 0, 0));
		}

		for entry in entries.iter_mut() {
			entry.run_length = reader.read_varint()? as u32;
		}

		for entry in entries.iter_mut() {
			entry.range.length = reader.read_varint()?;
		}

		for i in 0..num_entries {
			let tmp = reader.read_varint()?;
			if i > 0 && tmp == 0 {
				entries[i].range.offset = entries[i - 1].range.offset + entries[i - 1].range.length;
			} else {
				entries[i].range.offset = tmp - 1
			}
		}

		Ok(EntriesV3 { entries })
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn push(&mut self, entry: EntryV3) {
		self.entries.push(entry)
	}

	pub fn as_slice(&self) -> EntriesSliceV3 {
		EntriesSliceV3 { entries: &self.entries }
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

	pub fn as_directory(&self, target_root_len: usize) -> Result<Directory> {
		let entries: &EntriesSliceV3 = &self.as_slice();

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
			let d = build_roots_leaves(entries, leaf_size as usize)?;
			if d.root_bytes.len() <= target_root_len {
				return Ok(d);
			}
			leaf_size *= 1.2
		}

		fn build_roots_leaves(entries: &EntriesSliceV3, leaf_size: usize) -> Result<Directory> {
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
				leaves_bytes.write_all(serialized.as_slice())?;

				idx += leaf_size;
			}

			let root_bytes = root_entries.as_slice().serialize_entries()?;

			Ok(Directory {
				root_bytes,
				leaves_bytes: Blob::from(leaves_bytes),
			})
		}
	}
}

impl Default for EntriesV3 {
	fn default() -> Self {
		Self::new()
	}
}

impl From<&Blob> for EntriesV3 {
	fn from(blob: &Blob) -> Self {
		EntriesV3::from_blob(blob).unwrap()
	}
}

pub struct EntriesSliceV3<'a> {
	entries: &'a [EntryV3],
}

impl<'a> EntriesSliceV3<'a> {
	pub fn len(&self) -> usize {
		self.entries.len()
	}
	pub fn slice<T>(&self, range: T) -> EntriesSliceV3
	where
		T: SliceIndex<[EntryV3], Output = [EntryV3]>,
	{
		EntriesSliceV3 {
			entries: &self.entries[range],
		}
	}
	pub fn get(&self, index: usize) -> &EntryV3 {
		self.entries.get(index).unwrap()
	}
	pub fn serialize_entries(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new();
		let entries = self.entries;

		// Serialize the length of entries
		let len = entries.len() as u64;
		writer.write_varint(len)?;

		// Serialize TileID deltas
		let mut last_id: u64 = 0;
		for entry in entries {
			let delta = entry.tile_id - last_id;
			writer.write_varint(delta)?;
			last_id = entry.tile_id;
		}

		// Serialize RunLengths
		for entry in entries {
			writer.write_varint(entry.run_length as u64)?;
		}

		// Serialize Lengths
		for entry in entries {
			writer.write_varint(entry.range.length)?;
		}

		// Serialize Offsets
		for i in 0..entries.len() {
			let offset = if i > 0 && entries[i].range.offset == entries[i - 1].range.offset + entries[i - 1].range.length {
				0
			} else {
				entries[i].range.offset + 1 // add 1 to not conflict with 0
			};
			writer.write_varint(offset)?;
		}

		Ok(writer.into_blob())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Helper function to create sample entries
	fn create_entries() -> EntriesV3 {
		let mut entries = EntriesV3::new();
		entries.push(EntryV3::new(1, 100, 1000, 0)); // Example EntryV3::new(tile_id, offset, length, run_length)
		entries.push(EntryV3::new(2, 200, 1000, 1));
		entries.push(EntryV3::new(3, 300, 1000, 0));
		entries
	}

	#[test]
	fn serialize_entries() -> Result<()> {
		let entries = create_entries();
		let serialized = entries.as_slice().serialize_entries()?;
		assert_eq!(
			serialized.as_hex(),
			"03 01 01 01 00 01 00 e8 07 e8 07 e8 07 65 c9 01 ad 02"
		);

		let new_entries = EntriesV3::from_blob(&serialized)?;
		assert_eq!(entries, new_entries);

		Ok(())
	}

	#[test]
	fn test_find_tile() {
		let entries = create_entries();
		let entry = entries.find_tile(2).unwrap();
		assert_eq!(entry.tile_id, 2);
	}

	#[test]
	fn test_push_and_len() {
		let mut entries = EntriesV3::new();
		assert_eq!(entries.len(), 0);
		entries.push(EntryV3::new(1, 0, 0, 0));
		assert_eq!(entries.len(), 1);
	}

	#[test]
	fn test_as_directory() -> Result<()> {
		let entries = create_entries();
		let directory = entries.as_directory(1000)?; // Assuming 1000 is enough size for root
		assert!(!directory.root_bytes.is_empty());
		Ok(())
	}
}
