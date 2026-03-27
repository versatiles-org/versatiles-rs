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
