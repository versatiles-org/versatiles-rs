use crate::{
	container::pmtiles::types::pmblob::BlobReader,
	helper::compress_gzip,
	types::{Blob, ByteRange},
};
use anyhow::Result;
use std::{cmp::Ordering, fmt::Debug, slice::SliceIndex};

use super::pmblob::BlobWriter;

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

#[derive(Debug)]
pub struct EntriesV3 {
	entries: Vec<EntryV3>,
}

impl EntriesV3 {
	pub fn new() -> Self {
		Self { entries: Vec::new() }
	}

	pub fn deserialize(data: &Blob) -> Result<Self> {
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
}

impl From<&Blob> for EntriesV3 {
	fn from(blob: &Blob) -> Self {
		EntriesV3::deserialize(&blob).unwrap()
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

		compress_gzip(&writer.to_blob())
	}
}
