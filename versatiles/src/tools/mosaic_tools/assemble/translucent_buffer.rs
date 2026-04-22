//! Thread-safe buffer for translucent tiles awaiting further compositing.
//!
//! Tiles that are not yet fully opaque are stored here, keyed by their Hilbert index
//! (a space-filling curve that maps `(level, x, y)` to a single `u64`). When a new
//! tile arrives at the same coordinate, it is composited with the buffered tile.
//! Once opaque or when no more sources can contribute, the tile is flushed to the sink.
//!
//! The buffer is shared across `spawn_blocking` tasks via `Arc<TranslucentBuffer>`.
//! All methods acquire the internal `Mutex` for the duration of their operation.

use std::collections::HashMap;
use std::sync::Mutex;
use versatiles_container::Tile;
use versatiles_core::{TileCoord, utils::HilbertIndex};

/// Thread-safe buffer for translucent tiles awaiting further compositing.
///
/// Provides named operations over a `Mutex<HashMap<u64, (TileCoord, Tile)>>`,
/// hiding lock management from callers. The `u64` key is the tile's Hilbert index.
pub struct TranslucentBuffer {
	inner: Mutex<HashMap<u64, (TileCoord, Tile)>>,
}

impl TranslucentBuffer {
	pub fn new() -> Self {
		Self {
			inner: Mutex::new(HashMap::new()),
		}
	}

	#[cfg(test)]
	pub fn len(&self) -> usize {
		self.inner.lock().unwrap().len()
	}

	#[cfg(test)]
	pub fn clear(&self) {
		self.inner.lock().unwrap().clear();
	}

	/// Remove and return all entries.
	pub fn drain(&self) -> HashMap<u64, (TileCoord, Tile)> {
		self.inner.lock().expect("poisoned mutex").drain().collect()
	}

	/// Remove a tile by its Hilbert key, returning `(coord, tile)` if present.
	pub fn remove(&self, key: u64) -> Option<(TileCoord, Tile)> {
		self.inner.lock().expect("poisoned mutex").remove(&key)
	}

	/// Insert a tile. The key must be `coord.get_hilbert_index()`.
	pub fn insert(&self, coord: TileCoord, tile: Tile) -> anyhow::Result<()> {
		let key = coord.get_hilbert_index()?;
		self.inner.lock().expect("poisoned mutex").insert(key, (coord, tile));
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::{Blob, TileCompression, TileFormat};

	fn dummy_tile() -> Tile {
		Tile::from_blob(Blob::from(vec![0u8; 4]), TileCompression::Uncompressed, TileFormat::PNG)
	}

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	#[test]
	fn new_buffer_is_empty() {
		let buf = TranslucentBuffer::new();
		assert_eq!(buf.len(), 0);
	}

	#[test]
	fn insert_and_len() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(5, 1, 2), dummy_tile())?;
		assert_eq!(buf.len(), 1);
		buf.insert(coord(5, 3, 4), dummy_tile())?;
		assert_eq!(buf.len(), 2);
		Ok(())
	}

	#[test]
	fn insert_same_coord_overwrites() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(5, 1, 2), dummy_tile())?;
		buf.insert(coord(5, 1, 2), dummy_tile())?;
		assert_eq!(buf.len(), 1);
		Ok(())
	}

	#[test]
	fn remove_existing_key() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		let c = coord(5, 1, 2);
		buf.insert(c, dummy_tile())?;
		let key = c.get_hilbert_index()?;

		let removed = buf.remove(key);
		assert!(removed.is_some());
		assert_eq!(removed.unwrap().0, c);
		assert_eq!(buf.len(), 0);
		Ok(())
	}

	#[test]
	fn remove_missing_key() {
		let buf = TranslucentBuffer::new();
		assert!(buf.remove(12345).is_none());
	}

	#[test]
	fn clear_empties_buffer() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(3, 0, 0), dummy_tile())?;
		buf.insert(coord(3, 1, 1), dummy_tile())?;
		assert_eq!(buf.len(), 2);

		buf.clear();
		assert_eq!(buf.len(), 0);
		Ok(())
	}

	#[test]
	fn drain_returns_all_and_empties() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(4, 0, 0), dummy_tile())?;
		buf.insert(coord(4, 1, 0), dummy_tile())?;
		buf.insert(coord(4, 0, 1), dummy_tile())?;

		let drained = buf.drain();
		assert_eq!(drained.len(), 3);
		assert_eq!(buf.len(), 0);
		Ok(())
	}

	#[test]
	fn drain_on_empty_buffer() {
		let buf = TranslucentBuffer::new();
		let drained = buf.drain();
		assert!(drained.is_empty());
	}

	#[test]
	fn insert_then_drain_preserves_coords() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		let c1 = coord(3, 1, 2);
		let c2 = coord(3, 3, 4);
		buf.insert(c1, dummy_tile())?;
		buf.insert(c2, dummy_tile())?;

		let drained = buf.drain();
		let coords: std::collections::HashSet<TileCoord> = drained.values().map(|(c, _)| *c).collect();
		assert!(coords.contains(&c1));
		assert!(coords.contains(&c2));
		Ok(())
	}

	#[test]
	fn multiple_independent_buffers() -> anyhow::Result<()> {
		let buf1 = TranslucentBuffer::new();
		let buf2 = TranslucentBuffer::new();

		buf1.insert(coord(1, 0, 0), dummy_tile())?;
		buf1.insert(coord(1, 1, 0), dummy_tile())?;
		buf2.insert(coord(2, 0, 0), dummy_tile())?;

		assert_eq!(buf1.len(), 2);
		assert_eq!(buf2.len(), 1);

		buf1.clear();
		assert_eq!(buf1.len(), 0);
		assert_eq!(buf2.len(), 1); // buf2 unaffected
		Ok(())
	}

	#[test]
	fn remove_after_drain_returns_none() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		let c = coord(5, 1, 2);
		buf.insert(c, dummy_tile())?;
		let key = c.get_hilbert_index()?;

		let _ = buf.drain();
		assert!(buf.remove(key).is_none());
		Ok(())
	}

	#[test]
	fn insert_after_clear() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(1, 0, 0), dummy_tile())?;
		buf.clear();
		assert_eq!(buf.len(), 0);

		buf.insert(coord(2, 0, 0), dummy_tile())?;
		assert_eq!(buf.len(), 1);
		Ok(())
	}

	#[test]
	fn concurrent_insert_from_threads() -> anyhow::Result<()> {
		use std::sync::Arc;
		let buf = Arc::new(TranslucentBuffer::new());
		let mut handles = Vec::new();

		for i in 0..10u32 {
			let buf = Arc::clone(&buf);
			handles.push(std::thread::spawn(move || buf.insert(coord(5, i, 0), dummy_tile())));
		}

		for h in handles {
			h.join().unwrap()?;
		}
		assert_eq!(buf.len(), 10);
		Ok(())
	}
}
