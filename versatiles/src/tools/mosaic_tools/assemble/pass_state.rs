//! Multi-pass eviction state for memory-bounded assembly.
//!
//! When `--max-buffer-size` is set, the translucent tile buffer has a finite capacity.
//! If it fills up during a pass, [`PassState`] computes a horizontal cutline (`cut_y`)
//! in tile-coordinate space at the highest zoom level. Tiles north of (y < `cut_y`) are
//! evicted from the buffer and skipped by remaining sources in the current pass. After
//! the pass, a new pass processes only the evicted (northern) region.
//!
//! # Coordinate projection
//!
//! Tiles exist at different zoom levels with different coordinate ranges. To compare
//! them, tile y-coordinates are projected to the highest zoom level:
//! `projected_y = coord.y << (max_level - coord.level)`.
//! The cutline `cut_y` is defined at this highest resolution.
//!
//! When clipping pyramids or evicting tiles at a specific zoom level, `cut_y` is
//! projected back: `level_cut_y = cut_y >> (max_level - level)`. Due to integer
//! division, this projection is lossy at lower zoom levels. Tiles at levels where
//! `level_cut_y == 0` are never evicted because they cannot be assigned to a single
//! pass and would be lost.

use super::translucent_buffer::TranslucentBuffer;
use versatiles_core::{TileBBox, TileBBoxPyramid};

/// Manages multi-pass eviction state for memory-bounded assembly.
///
/// Created only when prescanned pyramids are available (either via `--optimize-order`
/// or implicitly via `--max-buffer-size`). When `max_buffer_size == 0`,
/// `max_buffer_tiles` is `usize::MAX` and eviction never triggers — the assembly
/// completes in a single pass.
pub struct PassState {
	/// Union of all source pyramids, clipped to the requested zoom range.
	/// Immutable after construction — used as the starting point for each pass.
	union_pyramid: TileBBoxPyramid,
	/// The region still to be processed in the current pass. Starts as a clone of
	/// `union_pyramid` and gets clipped southward during eviction, then clipped
	/// northward between passes.
	remaining_pyramid: TileBBoxPyramid,
	/// Highest zoom level in `union_pyramid`. Used to project coordinates.
	max_level: u8,
	/// Maximum number of tiles allowed in the translucent buffer before eviction.
	max_buffer_tiles: usize,
	/// Current horizontal cutline (at `max_level` resolution). Tiles with
	/// `projected_y < cut_y` are excluded from the current pass.
	/// Reset to 0 at the start of each pass.
	cut_y: u32,
}

impl PassState {
	/// Create a new `PassState` from prescanned pyramids.
	pub fn new(
		pyramids: &[TileBBoxPyramid],
		min_zoom: Option<u8>,
		max_zoom: Option<u8>,
		max_buffer_size: u64,
		tile_dim: u64,
	) -> Self {
		let union_pyramid = build_union_pyramid(pyramids, min_zoom, max_zoom);
		let max_level = union_pyramid.get_level_max().unwrap_or(0);
		let remaining_pyramid = union_pyramid.clone();

		let max_buffer_tiles = if max_buffer_size > 0 {
			usize::try_from(max_buffer_size / (tile_dim * tile_dim * 4)).unwrap_or(usize::MAX)
		} else {
			usize::MAX
		};

		Self {
			union_pyramid,
			remaining_pyramid,
			max_level,
			max_buffer_tiles,
			cut_y: 0,
		}
	}

	/// Reset state for a new pass.
	pub fn start_pass(&mut self) {
		self.cut_y = 0;
	}

	/// Intersect a source pyramid with the remaining region for this pass.
	pub fn clip_source_pyramid(&self, pyramid: &mut TileBBoxPyramid) {
		pyramid.intersect(&self.remaining_pyramid);
	}

	/// If the buffer exceeds the size limit, evict northern tiles and update the cutline.
	pub fn check_eviction(&mut self, buffer: &TranslucentBuffer) {
		let buf_len = buffer.len();
		if buf_len <= self.max_buffer_tiles {
			return;
		}

		let new_cut_y = buffer.compute_cut_y(self.max_buffer_tiles, self.max_level);
		if new_cut_y <= self.cut_y {
			return;
		}

		self.cut_y = new_cut_y;
		log::debug!(
			"buffer exceeded limit ({buf_len} > {}), evicting tiles north of cut_y={}",
			self.max_buffer_tiles,
			self.cut_y
		);

		// Remove tiles north of the cut from buffer
		let max_level = self.max_level;
		let cut_y = self.cut_y;
		let evicted = buffer.remove_tiles_where(|coord| {
			let level_cut_y = cut_y >> (max_level - coord.level);
			level_cut_y > 0 && coord.y < level_cut_y
		});
		log::debug!("evicted {} tiles, {} remaining in buffer", evicted.len(), buffer.len());

		// Clip remaining_pyramid so subsequent sources skip evicted region
		for level in 0..=self.max_level {
			let level_cut_y = self.cut_y >> (self.max_level - level);
			let bbox = self.remaining_pyramid.get_level_bbox(level);
			if !bbox.is_empty()
				&& let Ok(y_min) = bbox.y_min()
				&& y_min < level_cut_y
			{
				let mut new_bbox = *bbox;
				let _ = new_bbox.set_y_min(level_cut_y);
				self.remaining_pyramid.set_level_bbox(new_bbox);
			}
		}
	}

	/// Returns true if no eviction happened during this pass (all tiles processed).
	pub fn is_pass_complete(&self) -> bool {
		self.cut_y == 0
	}

	/// Prepare for the next pass: restrict to tiles north of the current cutline.
	pub fn prepare_next_pass(&mut self) {
		log::debug!(
			"pass complete, restarting for remaining tiles above cut_y={}",
			self.cut_y
		);
		self.remaining_pyramid = clip_pyramid_to_north(&self.union_pyramid, self.cut_y, self.max_level);
	}
}

/// Build a union pyramid from all prescanned pyramids, clipped to zoom range.
fn build_union_pyramid(pyramids: &[TileBBoxPyramid], min_zoom: Option<u8>, max_zoom: Option<u8>) -> TileBBoxPyramid {
	let mut u = TileBBoxPyramid::new_empty();
	for p in pyramids {
		u.include_pyramid(p);
	}
	if let Some(min) = min_zoom {
		u.set_level_min(min);
	}
	if let Some(max) = max_zoom {
		u.set_level_max(max);
	}
	u
}

/// Create a pyramid covering only tiles north of `cut_y` (for the next pass).
fn clip_pyramid_to_north(union_pyramid: &TileBBoxPyramid, cut_y: u32, max_level: u8) -> TileBBoxPyramid {
	let mut next = union_pyramid.clone();
	for level in 0..=max_level {
		let shift = max_level - level;
		let level_cut_y = cut_y >> shift;
		let bbox = next.get_level_bbox(level);
		if !bbox.is_empty() {
			if level_cut_y == 0 {
				next.set_level_bbox(TileBBox::new_empty(level).unwrap());
			} else {
				let mut new_bbox = *bbox;
				let _ = new_bbox.set_y_max(level_cut_y - 1);
				next.set_level_bbox(new_bbox);
			}
		}
	}
	next
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_container::Tile;
	use versatiles_core::{Blob, TileCompression, TileCoord, TileFormat};

	fn dummy_tile() -> Tile {
		Tile::from_blob(Blob::from(vec![0u8; 4]), TileCompression::Uncompressed, TileFormat::PNG)
	}

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	/// Create a pyramid covering (0,0)-(max,max) at the given level.
	fn full_pyramid_at(level: u8) -> TileBBoxPyramid {
		let mut p = TileBBoxPyramid::new_empty();
		p.set_level_bbox(TileBBox::new_full(level).unwrap());
		p
	}

	// --- build_union_pyramid ---

	#[test]
	fn build_union_pyramid_merges_levels() {
		let p1 = full_pyramid_at(5);
		let p2 = full_pyramid_at(8);
		let union = build_union_pyramid(&[p1, p2], None, None);
		assert!(!union.get_level_bbox(5).is_empty());
		assert!(!union.get_level_bbox(8).is_empty());
		assert!(union.get_level_bbox(3).is_empty());
	}

	#[test]
	fn build_union_pyramid_clips_zoom() {
		let mut p = TileBBoxPyramid::new_empty();
		p.set_level_bbox(TileBBox::new_full(5).unwrap());
		p.set_level_bbox(TileBBox::new_full(8).unwrap());
		p.set_level_bbox(TileBBox::new_full(10).unwrap());

		let union = build_union_pyramid(&[p], Some(8), Some(10));
		// Level 5 should be clipped away (below min_zoom=8)
		assert!(union.get_level_bbox(5).is_empty());
		assert!(!union.get_level_bbox(8).is_empty());
		assert!(!union.get_level_bbox(10).is_empty());
	}

	// --- clip_pyramid_to_north ---

	#[test]
	fn clip_pyramid_to_north_restricts_y() {
		let p = full_pyramid_at(10);
		let max_level = 10;
		let cut_y = 512; // at level 10

		let north = clip_pyramid_to_north(&p, cut_y, max_level);
		let bbox = north.get_level_bbox(10);
		assert!(!bbox.is_empty());
		// y_max should be cut_y - 1 = 511
		assert_eq!(bbox.y_max().unwrap(), 511);
	}

	#[test]
	fn clip_pyramid_to_north_empties_coarse_levels() {
		// At level 0, there's only tile (0,0). cut_y >> 10 = 0 for any cut_y < 1024.
		// level_cut_y == 0 means the level should be emptied.
		let mut p = TileBBoxPyramid::new_empty();
		p.set_level_bbox(TileBBox::new_full(0).unwrap());
		p.set_level_bbox(TileBBox::new_full(10).unwrap());

		let north = clip_pyramid_to_north(&p, 512, 10);
		assert!(
			north.get_level_bbox(0).is_empty(),
			"level 0 should be empty when level_cut_y == 0"
		);
		assert!(!north.get_level_bbox(10).is_empty());
	}

	// --- PassState::new ---

	#[test]
	fn new_with_no_buffer_limit() {
		let p = full_pyramid_at(8);
		let ps = PassState::new(&[p], None, None, 0, 256);
		// max_buffer_size=0 → max_buffer_tiles=usize::MAX
		assert_eq!(ps.max_buffer_tiles, usize::MAX);
		assert_eq!(ps.max_level, 8);
		assert!(ps.is_pass_complete());
	}

	#[test]
	fn new_with_buffer_limit() {
		let p = full_pyramid_at(10);
		// 256x256 tiles, 4 bytes per pixel = 262144 bytes per tile
		// max_buffer_size = 1_000_000 → max_buffer_tiles = 1_000_000 / 262144 = 3
		let ps = PassState::new(&[p], None, None, 1_000_000, 256);
		assert_eq!(ps.max_buffer_tiles, 3);
	}

	// --- PassState::start_pass ---

	#[test]
	fn start_pass_resets_cut_y() {
		let p = full_pyramid_at(10);
		let mut ps = PassState::new(&[p], None, None, 1_000_000, 256);
		ps.cut_y = 42;
		ps.start_pass();
		assert_eq!(ps.cut_y, 0);
		assert!(ps.is_pass_complete());
	}

	// --- PassState::clip_source_pyramid ---

	#[test]
	fn clip_source_pyramid_intersects() {
		let p = full_pyramid_at(10);
		let ps = PassState::new(&[p], None, None, 0, 256);
		// Simulate eviction having clipped remaining_pyramid
		let _ = ps.remaining_pyramid.get_level_bbox(10);
		let mut source = full_pyramid_at(10);
		ps.clip_source_pyramid(&mut source);
		// After intersection with full remaining_pyramid, source should still be full
		assert!(!source.get_level_bbox(10).is_empty());
	}

	// --- PassState::check_eviction ---

	#[test]
	fn check_eviction_no_op_when_under_limit() -> anyhow::Result<()> {
		let p = full_pyramid_at(10);
		let mut ps = PassState::new(&[p], None, None, 100_000_000, 256);
		let buf = TranslucentBuffer::new();
		buf.insert(coord(10, 0, 0), dummy_tile())?;

		ps.check_eviction(&buf);
		assert!(ps.is_pass_complete(), "no eviction should happen when buffer is small");
		assert_eq!(buf.len(), 1);
		Ok(())
	}

	#[test]
	fn check_eviction_triggers_when_over_limit() -> anyhow::Result<()> {
		let p = full_pyramid_at(10);
		// max_buffer_tiles = 2 (very small)
		let mut ps = PassState::new(&[p], None, None, 2 * 256 * 256 * 4, 256);
		assert_eq!(ps.max_buffer_tiles, 2);

		let buf = TranslucentBuffer::new();
		// Insert 4 tiles at level 10 with increasing y
		buf.insert(coord(10, 0, 100), dummy_tile())?;
		buf.insert(coord(10, 0, 200), dummy_tile())?;
		buf.insert(coord(10, 0, 300), dummy_tile())?;
		buf.insert(coord(10, 0, 400), dummy_tile())?;

		ps.check_eviction(&buf);
		assert!(!ps.is_pass_complete(), "eviction should have set cut_y > 0");
		// Northern tiles should have been evicted, keeping ~2 southern tiles
		assert!(buf.len() <= 2, "buffer should be at or below limit after eviction");
		Ok(())
	}

	// --- PassState::prepare_next_pass ---

	#[test]
	fn prepare_next_pass_restricts_to_north() -> anyhow::Result<()> {
		let p = full_pyramid_at(10);
		let mut ps = PassState::new(&[p], None, None, 2 * 256 * 256 * 4, 256);

		// Simulate eviction
		let buf = TranslucentBuffer::new();
		for y in 0..10 {
			buf.insert(coord(10, 0, y * 100), dummy_tile())?;
		}
		ps.check_eviction(&buf);
		assert!(!ps.is_pass_complete());

		let cut_y = ps.cut_y;
		ps.prepare_next_pass();

		// After prepare_next_pass, remaining_pyramid at level 10 should only cover y < cut_y
		let bbox = ps.remaining_pyramid.get_level_bbox(10);
		assert!(!bbox.is_empty());
		assert_eq!(bbox.y_max().unwrap(), (cut_y - 1).min(bbox.y_max().unwrap()));
		Ok(())
	}

	// --- is_pass_complete ---

	#[test]
	fn is_pass_complete_without_eviction() {
		let p = full_pyramid_at(5);
		let ps = PassState::new(&[p], None, None, 0, 256);
		assert!(ps.is_pass_complete());
	}
}
