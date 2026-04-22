use super::{Directory, EntryV3};
use anyhow::{Result, bail};
use std::{
	cmp::Ordering,
	io::Write,
	slice::{Iter, SliceIndex},
};
use versatiles_core::{
	Blob, ByteRange, TileCompression,
	compression::compress,
	io::{ValueReader, ValueReaderSlice, ValueWriter, ValueWriterBlob},
	utils::float_to_int,
};

/// A collection of `EntryV3` that provides various utility functions
/// for handling tile data entries, including serialization, deserialization,
/// and querying.
#[derive(Debug, PartialEq)]
pub struct EntriesV3 {
	entries: Vec<EntryV3>,
}

impl EntriesV3 {
	/// Constructs a new, empty `EntriesV3`.
	pub fn new() -> Self {
		Self { entries: Vec::new() }
	}

	/// Deserializes a `Blob` into an `EntriesV3` instance.
	///
	/// # Arguments
	/// * `data` - A reference to the `Blob` containing the serialized entries.
	///
	/// # Errors
	/// Returns an error if the `Blob` format is incorrect or the data cannot be parsed.
	///
	/// # Panics
	/// Panics if the number of entries exceeds 10 billion, which is considered an error.
	pub fn from_blob(data: &Blob) -> Result<Self> {
		let mut entries: Vec<EntryV3> = Vec::new();
		let mut reader = ValueReaderSlice::new_le(data.as_slice());

		let num_entries = usize::try_from(reader.read_varint()?)?;

		if num_entries > 10_000_000_000 {
			bail!("there is something wrong: PMTiles with more then 10 billion tiles?")
		}

		let mut last_id: u64 = 0;

		for _ in 0..num_entries {
			let diff = reader.read_varint()?;
			last_id += diff;
			entries.push(EntryV3::new(last_id, ByteRange::empty(), 0));
		}

		for entry in &mut entries {
			entry.run_length = u32::try_from(reader.read_varint()?)?;
		}

		for entry in &mut entries {
			entry.range.length = reader.read_varint()?;
		}

		for i in 0..num_entries {
			let tmp = reader.read_varint()?;
			if i > 0 && tmp == 0 {
				entries[i].range.offset = entries[i - 1].range.offset + entries[i - 1].range.length;
			} else {
				entries[i].range.offset = tmp - 1;
			}
		}

		Ok(EntriesV3 { entries })
	}

	/// Returns the number of entries in the collection.
	pub fn len(&self) -> usize {
		self.entries.len()
	}

	/// Adds a new `EntryV3` to the collection.
	///
	/// # Arguments
	/// * `entry` - The `EntryV3` to be added.
	pub fn push(&mut self, entry: EntryV3) {
		self.entries.push(entry);
	}

	/// Returns a slice view into the entries.
	pub fn as_slice(&self) -> EntriesSliceV3<'_> {
		EntriesSliceV3 { entries: &self.entries }
	}

	/// Iterates over the entries.
	pub fn iter(&self) -> Iter<'_, EntryV3> {
		self.entries.iter()
	}

	/// Finds an `EntryV3` by its tile ID using a binary search.
	///
	/// # Arguments
	/// * `tile_id` - The tile ID to search for.
	///
	/// Returns `Some(EntryV3)` if found, or `None` if no entry matches the tile ID.
	pub fn find_tile(&self, tile_id: u64) -> Option<EntryV3> {
		let mut m: i64 = 0;
		let mut n: i64 = i64::try_from(self.entries.len()).expect("entries length fits in i64") - 1;

		while m <= n {
			let k = (n + m) >> 1;
			let k_idx = usize::try_from(k).expect("k is non-negative and within entries len");
			let entry_id = self.entries[k_idx].tile_id;
			match tile_id.cmp(&entry_id) {
				Ordering::Greater => m = k + 1,
				Ordering::Less => n = k - 1,
				Ordering::Equal => return Some(self.entries[k_idx]),
			}
		}

		// at this point, m > n
		if n >= 0 {
			let entry = self.entries[usize::try_from(n).expect("n is non-negative here")];
			if entry.run_length == 0 {
				return Some(entry);
			}
			if tile_id - entry.tile_id < u64::from(entry.run_length) {
				return Some(entry);
			}
		}

		None
	}

	/// Converts the entries to a directory format, potentially compressing them,
	/// based on the provided root length and compression settings.
	///
	/// # Arguments
	/// * `target_root_len` - The maximum size of the root directory in bytes.
	/// * `compression` - The compression method to be applied.
	///
	/// # Errors
	/// Returns an error if the entries cannot be serialized or compressed as specified.
	pub fn build_directory(&mut self, target_root_len: u64, compression: TileCompression) -> Result<Directory> {
		self.entries.sort_by_cached_key(|e| e.tile_id);
		let entries: &EntriesSliceV3 = &self.as_slice();

		if entries.len() < 16384 {
			let root_bytes = compress(entries.serialize_entries()?, &compression)?;
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
			let d = build_roots_leaves(entries, float_to_int(leaf_size)?, compression)?;
			if d.root_bytes.len() <= target_root_len {
				return Ok(d);
			}
			leaf_size *= 1.2;
		}

		fn build_roots_leaves(
			entries: &EntriesSliceV3,
			leaf_size: usize,
			compression: TileCompression,
		) -> Result<Directory> {
			let mut root_entries = EntriesV3::new();
			let mut leaves_bytes: Vec<u8> = Vec::new();

			let mut idx: usize = 0;
			while idx < entries.len() {
				let mut end = idx + leaf_size;
				if idx + leaf_size > entries.len() {
					end = entries.len();
				}
				let serialized = compress(entries.slice(idx..end).serialize_entries()?, &compression)?;

				root_entries.push(EntryV3::new(
					entries.get(idx).tile_id,
					ByteRange::new(leaves_bytes.len() as u64, serialized.len() as u64),
					0,
				));
				leaves_bytes.write_all(serialized.as_slice())?;

				idx += leaf_size;
			}

			let root_bytes = compress(root_entries.as_slice().serialize_entries()?, &compression)?;

			Ok(Directory {
				root_bytes,
				leaves_bytes: Blob::from(leaves_bytes),
			})
		}
	}

	pub fn tile_count(&self) -> u64 {
		self.entries.iter().map(|e| u64::from(e.run_length.max(1))).sum()
	}

	/// Merges consecutive entries that point to the same byte range into
	/// single entries with accumulated `run_length`. Entries must be sorted
	/// by `tile_id` before calling this method.
	pub fn merge_runs(&mut self) {
		if self.entries.len() <= 1 {
			return;
		}
		let mut merged = Vec::with_capacity(self.entries.len());
		let mut current = self.entries[0];
		for &entry in &self.entries[1..] {
			if entry.range == current.range && entry.tile_id == current.tile_id + u64::from(current.run_length) {
				current.run_length += entry.run_length;
			} else {
				merged.push(current);
				current = entry;
			}
		}
		merged.push(current);
		self.entries = merged;
	}
}

impl Default for EntriesV3 {
	/// Provides a default instance of `EntriesV3`, which is empty.
	fn default() -> Self {
		Self::new()
	}
}

impl From<&Blob> for EntriesV3 {
	/// Creates an `EntriesV3` from a `Blob` by deserializing it.
	///
	/// # Panics
	/// Panics if deserialization fails.
	fn from(blob: &Blob) -> Self {
		EntriesV3::from_blob(blob).expect("valid pmtiles entries blob")
	}
}

/// A slice of `EntryV3`, supporting partial views into `EntriesV3`.
pub struct EntriesSliceV3<'a> {
	entries: &'a [EntryV3],
}

impl EntriesSliceV3<'_> {
	/// Returns the number of entries in the slice.
	pub fn len(&self) -> usize {
		self.entries.len()
	}

	/// Creates a sub-slice of entries.
	///
	/// # Arguments
	/// * `range` - The range within the current slice to create a sub-slice from.
	pub fn slice<T>(&self, range: T) -> EntriesSliceV3<'_>
	where
		T: SliceIndex<[EntryV3], Output = [EntryV3]>,
	{
		EntriesSliceV3 {
			entries: &self.entries[range],
		}
	}

	/// Retrieves an entry by its index.
	///
	/// # Arguments
	/// * `index` - The index of the entry to retrieve.
	///
	/// Returns a reference to the `EntryV3` at the specified index.
	pub fn get(&self, index: usize) -> &EntryV3 {
		self.entries.get(index).expect("index within entries slice")
	}

	/// Serializes the entries slice into a `Blob`.
	///
	/// # Errors
	/// Returns an error if any part of the serialization process fails.
	pub fn serialize_entries(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();
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
			writer.write_varint(u64::from(entry.run_length))?;
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
		entries.push(EntryV3::new(1, ByteRange::new(100, 100), 0)); // Example EntryV3::new(tile_id, offset, length, run_length)
		entries.push(EntryV3::new(2, ByteRange::new(200, 100), 1));
		entries.push(EntryV3::new(3, ByteRange::new(300, 100), 0));
		entries
	}

	#[test]
	fn serialize_entries() -> Result<()> {
		let entries = create_entries();
		let serialized = entries.as_slice().serialize_entries()?;
		assert_eq!(serialized.to_hex(), "03 01 01 01 00 01 00 64 64 64 65 00 00");

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
		entries.push(EntryV3::new(1, ByteRange::new(0, 0), 0));
		assert_eq!(entries.len(), 1);
	}

	#[test]
	fn test_build_directory() -> Result<()> {
		let mut entries = create_entries();
		let directory = entries.build_directory(1000, TileCompression::Uncompressed)?; // Assuming 1000 is enough size for root
		assert!(!directory.root_bytes.is_empty());
		Ok(())
	}

	/// Helper function to create and fill `EntriesV3` with a predetermined number of entries.
	fn create_filled_entries(num: u64) -> EntriesV3 {
		let mut entries = EntriesV3::new();
		for i in 0..num {
			entries.push(EntryV3::new(i, ByteRange::new(i * 100, 100), 1));
		}
		entries
	}

	#[test]
	fn test_serialization_deserialization_integrity() -> Result<()> {
		let entries = create_filled_entries(10);
		let blob = entries.as_slice().serialize_entries()?;
		let deserialized_entries = EntriesV3::from_blob(&blob)?;
		assert_eq!(entries, deserialized_entries);
		Ok(())
	}

	#[test]
	fn test_boundary_conditions() -> Result<()> {
		let entries = create_filled_entries(0);
		assert_eq!(entries.len(), 0);

		let blob = entries.as_slice().serialize_entries()?;
		let deserialized_entries = EntriesV3::from_blob(&blob)?;
		assert_eq!(deserialized_entries.len(), 0);
		Ok(())
	}

	#[test]
	fn test_large_dataset_find_tile() {
		let entries = create_filled_entries(1_000_000);
		assert!(entries.find_tile(999_999).is_some());
		assert!(entries.find_tile(1_000_000).is_none());
		assert_eq!(entries.len(), 1_000_000);
	}

	/// Verifies that `EntriesV3` can handle the maximum allowed number of entries without panicking.
	#[test]
	fn test_excessive_entries_panic() {
		let mut writer = ValueWriterBlob::new_le();
		// Mocking an excessively large number of entries, e.g., 10 billion + 1
		writer.write_varint(10_000_000_001).unwrap();
		let blob = writer.into_blob();
		assert_eq!(
			EntriesV3::from_blob(&blob).unwrap_err().to_string(),
			"there is something wrong: PMTiles with more then 10 billion tiles?"
		);
	}

	#[test]
	fn test_tile_count_sums_run_lengths() {
		let mut entries = EntriesV3::new();
		entries.push(EntryV3::new(0, ByteRange::new(0, 100), 3));
		entries.push(EntryV3::new(3, ByteRange::new(100, 100), 5));
		entries.push(EntryV3::new(8, ByteRange::new(200, 100), 1));
		assert_eq!(entries.tile_count(), 9);
		assert_eq!(entries.len(), 3);
	}

	#[test]
	fn test_tile_count_run_length_zero_counts_as_one() {
		let mut entries = EntriesV3::new();
		entries.push(EntryV3::new(0, ByteRange::new(0, 100), 0));
		assert_eq!(entries.tile_count(), 1);
	}

	#[test]
	fn test_merge_runs_consecutive_same_range() {
		let mut entries = EntriesV3::new();
		entries.push(EntryV3::new(0, ByteRange::new(0, 100), 1));
		entries.push(EntryV3::new(1, ByteRange::new(0, 100), 1));
		entries.push(EntryV3::new(2, ByteRange::new(0, 100), 1));
		entries.merge_runs();
		assert_eq!(entries.len(), 1);
		assert_eq!(entries.entries[0].run_length, 3);
		assert_eq!(entries.tile_count(), 3);
	}

	#[test]
	fn test_merge_runs_different_ranges_stay_separate() {
		let mut entries = EntriesV3::new();
		entries.push(EntryV3::new(0, ByteRange::new(0, 100), 1));
		entries.push(EntryV3::new(1, ByteRange::new(100, 100), 1));
		entries.push(EntryV3::new(2, ByteRange::new(200, 100), 1));
		entries.merge_runs();
		assert_eq!(entries.len(), 3);
	}

	#[test]
	fn test_merge_runs_empty_and_single() {
		let mut empty = EntriesV3::new();
		empty.merge_runs();
		assert_eq!(empty.len(), 0);

		let mut single = EntriesV3::new();
		single.push(EntryV3::new(0, ByteRange::new(0, 100), 1));
		single.merge_runs();
		assert_eq!(single.len(), 1);
	}

	#[test]
	fn test_merge_runs_mixed() {
		let mut entries = EntriesV3::new();
		// Run of 3 with same range
		entries.push(EntryV3::new(0, ByteRange::new(0, 100), 1));
		entries.push(EntryV3::new(1, ByteRange::new(0, 100), 1));
		entries.push(EntryV3::new(2, ByteRange::new(0, 100), 1));
		// Different range
		entries.push(EntryV3::new(3, ByteRange::new(100, 50), 1));
		// Run of 2 with same range
		entries.push(EntryV3::new(4, ByteRange::new(0, 100), 1));
		entries.push(EntryV3::new(5, ByteRange::new(0, 100), 1));
		entries.merge_runs();
		assert_eq!(entries.len(), 3);
		assert_eq!(entries.entries[0].run_length, 3);
		assert_eq!(entries.entries[1].run_length, 1);
		assert_eq!(entries.entries[2].run_length, 2);
		assert_eq!(entries.tile_count(), 6);
	}

	/// Tests the build_directory function for correct directory structure creation
	#[test]
	fn test_build_directory_structure() -> Result<()> {
		let mut entries = create_filled_entries(500); // A reasonable number of entries for testing
		let directory = entries.build_directory(1024, TileCompression::Uncompressed)?; // Assuming a small root directory size

		assert!(
			!directory.root_bytes.is_empty(),
			"Directory root bytes should not be empty"
		);
		assert!(
			!directory.leaves_bytes.is_empty(),
			"Directory leaves bytes should be non-zero for valid entries"
		);

		Ok(())
	}
}
