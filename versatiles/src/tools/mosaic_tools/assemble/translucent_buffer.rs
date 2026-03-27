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

	pub fn len(&self) -> usize {
		self.inner.lock().unwrap().len()
	}

	pub fn clear(&self) {
		self.inner.lock().unwrap().clear();
	}

	/// Remove and return all entries.
	pub fn drain(&self) -> HashMap<u64, (TileCoord, Tile)> {
		self.inner.lock().unwrap().drain().collect()
	}

	/// Remove a tile by its Hilbert key, returning `(coord, tile)` if present.
	pub fn remove(&self, key: u64) -> Option<(TileCoord, Tile)> {
		self.inner.lock().unwrap().remove(&key)
	}

	/// Insert a tile. The key must be `coord.get_hilbert_index()`.
	pub fn insert(&self, coord: TileCoord, tile: Tile) -> anyhow::Result<()> {
		let key = coord.get_hilbert_index()?;
		self.inner.lock().unwrap().insert(key, (coord, tile));
		Ok(())
	}

	/// Remove all tiles matching the predicate and return them.
	pub fn remove_tiles_where(&self, mut pred: impl FnMut(&TileCoord) -> bool) -> Vec<(TileCoord, Tile)> {
		let mut buf = self.inner.lock().unwrap();
		let keys: Vec<u64> = buf
			.iter()
			.filter(|(_, (coord, _))| pred(coord))
			.map(|(&k, _)| k)
			.collect();
		keys.iter().filter_map(|k| buf.remove(k)).collect()
	}

	/// Compute the `cut_y` value (at `max_level` resolution) that would keep
	/// approximately `max_tiles` southern (highest-y) tiles in the buffer.
	///
	/// Returns 0 if all tiles fit within `max_tiles`.
	pub fn compute_cut_y(&self, max_tiles: usize, max_level: u8) -> u32 {
		let buf = self.inner.lock().unwrap();
		let mut projected_ys: Vec<u32> = buf
			.values()
			.map(|(coord, _)| coord.y << (max_level - coord.level))
			.collect();
		projected_ys.sort_unstable();
		if projected_ys.len() > max_tiles {
			projected_ys[projected_ys.len() - max_tiles]
		} else {
			0
		}
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
	fn remove_tiles_where_filters_correctly() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(5, 0, 0), dummy_tile())?;
		buf.insert(coord(5, 1, 0), dummy_tile())?;
		buf.insert(coord(5, 2, 0), dummy_tile())?;
		buf.insert(coord(5, 3, 0), dummy_tile())?;

		// Remove tiles with x < 2
		let removed = buf.remove_tiles_where(|c| c.x < 2);
		assert_eq!(removed.len(), 2);
		assert_eq!(buf.len(), 2);
		Ok(())
	}

	#[test]
	fn remove_tiles_where_none_match() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		buf.insert(coord(5, 10, 10), dummy_tile())?;

		let removed = buf.remove_tiles_where(|c| c.x > 100);
		assert_eq!(removed.len(), 0);
		assert_eq!(buf.len(), 1);
		Ok(())
	}

	#[test]
	fn compute_cut_y_all_fit() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		// 3 tiles at level 10
		buf.insert(coord(10, 0, 100), dummy_tile())?;
		buf.insert(coord(10, 0, 200), dummy_tile())?;
		buf.insert(coord(10, 0, 300), dummy_tile())?;

		// max_tiles=5 > 3 tiles, so all fit
		assert_eq!(buf.compute_cut_y(5, 10), 0);
		Ok(())
	}

	#[test]
	fn compute_cut_y_eviction_needed() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		// 4 tiles at level 10, y values: 100, 200, 300, 400
		buf.insert(coord(10, 0, 100), dummy_tile())?;
		buf.insert(coord(10, 0, 200), dummy_tile())?;
		buf.insert(coord(10, 0, 300), dummy_tile())?;
		buf.insert(coord(10, 0, 400), dummy_tile())?;

		// Keep 2 tiles (southern = highest y). cut_y should be at projected_ys[4-2] = projected_ys[2]
		// All at level 10 with max_level=10, so projected_y = y. Sorted: [100, 200, 300, 400]
		// cut_y = projected_ys[2] = 300
		let cut_y = buf.compute_cut_y(2, 10);
		assert_eq!(cut_y, 300);
		Ok(())
	}

	#[test]
	fn compute_cut_y_with_level_projection() -> anyhow::Result<()> {
		let buf = TranslucentBuffer::new();
		// Tile at level 8, y=5. With max_level=10, projected_y = 5 << 2 = 20
		buf.insert(coord(8, 0, 5), dummy_tile())?;
		// Tile at level 10, y=100. projected_y = 100 << 0 = 100
		buf.insert(coord(10, 0, 100), dummy_tile())?;

		// Keep 1 tile (highest y = southernmost). Sorted projected: [20, 100].
		// cut_y = projected_ys[2-1] = 100. Tiles with projected_y < 100 are evicted.
		let cut_y = buf.compute_cut_y(1, 10);
		assert_eq!(cut_y, 100);
		Ok(())
	}
}
