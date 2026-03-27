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
	/// Cutline from the previous pass. Used for progress calculation.
	/// `u32::MAX` before the first pass (nothing completed yet).
	prev_cut_y: u32,
	/// Per-source pyramids clipped to zoom range, used for progress calculation.
	source_pyramids: Vec<TileBBoxPyramid>,
	/// Total tiles across all sources (zoom-clipped). Fixed progress bar total.
	total_tiles: u64,
}

impl PassState {
	/// Create a new `PassState` from prescanned, zoom-clipped pyramids.
	pub fn new(pyramids: &[TileBBoxPyramid], max_buffer_size: u64, tile_dim: u64) -> Self {
		let union_pyramid = build_union_pyramid(pyramids);
		let max_level = union_pyramid.get_level_max().unwrap_or(0);
		let remaining_pyramid = union_pyramid.clone();

		let max_buffer_tiles = if max_buffer_size > 0 {
			usize::try_from(max_buffer_size / (tile_dim * tile_dim * 4)).unwrap_or(usize::MAX)
		} else {
			usize::MAX
		};

		let source_pyramids = pyramids.to_vec();
		let total_tiles: u64 = source_pyramids.iter().map(TileBBoxPyramid::count_tiles).sum();

		Self {
			union_pyramid,
			remaining_pyramid,
			max_level,
			max_buffer_tiles,
			cut_y: 0,
			prev_cut_y: u32::MAX,
			source_pyramids,
			total_tiles,
		}
	}

	/// Total tiles across all sources (zoom-clipped). Used as the progress bar total.
	pub fn total_tiles(&self) -> u64 {
		self.total_tiles
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
		self.prev_cut_y = self.cut_y;
		self.remaining_pyramid = clip_pyramid_to_north(&self.union_pyramid, self.cut_y, self.max_level);
	}

	/// Compute the current progress position (number of completed tiles).
	///
	/// For sources already processed in this pass (`<= current_pos` in source_order),
	/// counts tiles south of the current `cut_y`. For sources not yet processed,
	/// counts tiles south of the previous pass's `cut_y`.
	///
	/// This may decrease slightly when `cut_y` increases during eviction, honestly
	/// reflecting that evicted tiles need reprocessing.
	pub fn compute_progress(&self, source_order: &[usize], current_pos: usize) -> u64 {
		let mut position = 0u64;
		for (i, &idx) in source_order.iter().enumerate() {
			let cut_y = if i <= current_pos { self.cut_y } else { self.prev_cut_y };
			position += count_tiles_south_of(&self.source_pyramids[idx], cut_y, self.max_level);
		}
		position
	}
}

/// Count tiles in a pyramid that are south of `cut_y` (i.e. `y >= level_cut_y` at each level).
///
/// When `cut_y == 0`, all tiles are south → returns the full count.
/// When `cut_y == u32::MAX`, no tiles are south → returns 0.
fn count_tiles_south_of(pyramid: &TileBBoxPyramid, cut_y: u32, max_level: u8) -> u64 {
	if cut_y == 0 {
		return pyramid.count_tiles();
	}
	let mut clipped = pyramid.clone();
	for level in 0..=max_level {
		let level_cut_y = cut_y >> (max_level - level);
		let bbox = clipped.get_level_bbox(level);
		if bbox.is_empty() {
			continue;
		}
		// If level_cut_y exceeds the bbox's y_max, all tiles are north → empty this level
		if bbox.y_max().is_ok_and(|y_max| level_cut_y > y_max) {
			clipped.set_level_bbox(TileBBox::new_empty(level).unwrap());
		} else if bbox.y_min().is_ok_and(|y_min| y_min < level_cut_y) {
			let mut new_bbox = *bbox;
			let _ = new_bbox.set_y_min(level_cut_y);
			clipped.set_level_bbox(new_bbox);
		}
	}
	clipped.count_tiles()
}

/// Build a union pyramid from all prescanned (already zoom-clipped) pyramids.
fn build_union_pyramid(pyramids: &[TileBBoxPyramid]) -> TileBBoxPyramid {
	let mut u = TileBBoxPyramid::new_empty();
	for p in pyramids {
		u.include_pyramid(p);
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
		let union = build_union_pyramid(&[p1, p2]);
		assert!(!union.get_level_bbox(5).is_empty());
		assert!(!union.get_level_bbox(8).is_empty());
		assert!(union.get_level_bbox(3).is_empty());
	}

	#[test]
	fn build_union_pyramid_from_pre_clipped() {
		let mut p = TileBBoxPyramid::new_empty();
		p.set_level_bbox(TileBBox::new_full(5).unwrap());
		p.set_level_bbox(TileBBox::new_full(8).unwrap());
		p.set_level_bbox(TileBBox::new_full(10).unwrap());

		// Pre-clip to zoom 8..=10 (as mod.rs now does before calling PassState)
		p.set_level_min(8);
		p.set_level_max(10);

		let union = build_union_pyramid(&[p]);
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
		let ps = PassState::new(&[p], 0, 256);
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
		let ps = PassState::new(&[p], 1_000_000, 256);
		assert_eq!(ps.max_buffer_tiles, 3);
	}

	// --- PassState::start_pass ---

	#[test]
	fn start_pass_resets_cut_y() {
		let p = full_pyramid_at(10);
		let mut ps = PassState::new(&[p], 1_000_000, 256);
		ps.cut_y = 42;
		ps.start_pass();
		assert_eq!(ps.cut_y, 0);
		assert!(ps.is_pass_complete());
	}

	// --- PassState::clip_source_pyramid ---

	#[test]
	fn clip_source_pyramid_intersects() {
		let p = full_pyramid_at(10);
		let ps = PassState::new(&[p], 0, 256);
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
		let mut ps = PassState::new(&[p], 100_000_000, 256);
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
		let mut ps = PassState::new(&[p], 2 * 256 * 256 * 4, 256);
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
		let mut ps = PassState::new(&[p], 2 * 256 * 256 * 4, 256);

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
		let ps = PassState::new(&[p], 0, 256);
		assert!(ps.is_pass_complete());
	}

	// --- count_tiles_south_of ---

	#[test]
	fn count_tiles_south_of_zero_returns_all() {
		let p = full_pyramid_at(4); // 16x16 = 256 tiles
		assert_eq!(count_tiles_south_of(&p, 0, 4), 256);
	}

	#[test]
	fn count_tiles_south_of_max_returns_zero() {
		let p = full_pyramid_at(4);
		assert_eq!(count_tiles_south_of(&p, u32::MAX, 4), 0);
	}

	#[test]
	fn count_tiles_south_of_half() {
		let p = full_pyramid_at(4); // 16x16 tiles, y: 0..15
		// cut_y = 8 at level 4 → keep y >= 8 → 8 rows × 16 cols = 128 tiles
		assert_eq!(count_tiles_south_of(&p, 8, 4), 128);
	}

	// --- compute_progress ---

	#[test]
	fn compute_progress_no_eviction() {
		// Two sources, each with a small pyramid at level 2 (4x4 = 16 tiles)
		let p1 = full_pyramid_at(2);
		let p2 = full_pyramid_at(2);
		let ps = PassState::new(&[p1, p2], 0, 256);
		let order = vec![0, 1];

		assert_eq!(ps.total_tiles(), 32); // 16 + 16

		// After processing source 0 (cut_y=0): source 0 = all 16, source 1 = 0 (prev_cut_y=MAX)
		assert_eq!(ps.compute_progress(&order, 0), 16);

		// After processing source 1: both sources = all tiles
		assert_eq!(ps.compute_progress(&order, 1), 32);
	}

	#[test]
	fn compute_progress_with_eviction() -> anyhow::Result<()> {
		let p = full_pyramid_at(4); // 16x16 = 256 tiles
		// max_buffer_tiles = 2, very small → eviction guaranteed
		let mut ps = PassState::new(&[p.clone(), p], 2 * 256 * 256 * 4, 256);
		let order = vec![0, 1];

		let total = ps.total_tiles();
		assert_eq!(total, 512); // 256 + 256

		// Simulate: process source 0, trigger eviction
		let buf = TranslucentBuffer::new();
		for y in 0..16 {
			for x in 0..16 {
				buf.insert(coord(4, x, y), dummy_tile())?;
			}
		}
		ps.check_eviction(&buf);
		assert!(!ps.is_pass_complete());

		// Progress after source 0: should be less than 256 (some tiles evicted)
		let progress_after_s0 = ps.compute_progress(&order, 0);
		assert!(progress_after_s0 < 256, "eviction should reduce progress for source 0");
		assert!(progress_after_s0 > 0, "some tiles should still be south of cut_y");

		// Source 1 not yet processed → contributes 0 (prev_cut_y = MAX)
		// So total progress = only source 0's south tiles
		assert_eq!(ps.compute_progress(&order, 0), progress_after_s0);

		Ok(())
	}

	#[test]
	fn compute_progress_after_prepare_next_pass() -> anyhow::Result<()> {
		let p = full_pyramid_at(4); // 256 tiles
		let mut ps = PassState::new(&[p.clone(), p], 2 * 256 * 256 * 4, 256);
		let order = vec![0, 1];

		// Trigger eviction
		let buf = TranslucentBuffer::new();
		for y in 0..16 {
			for x in 0..16 {
				buf.insert(coord(4, x, y), dummy_tile())?;
			}
		}
		ps.check_eviction(&buf);
		let cut_y = ps.cut_y;
		assert!(cut_y > 0);

		// Simulate end of pass 1: both sources processed
		let progress_end_pass1 = ps.compute_progress(&order, 1);

		// Prepare next pass
		ps.prepare_next_pass();
		ps.start_pass();

		// After start of pass 2, before any source processed:
		// All sources use prev_cut_y (the old cut_y) → same as end of pass 1
		// But compute_progress with current_pos would need a "before any source" state.
		// After processing source 0 in pass 2 (cut_y=0 → all tiles south):
		// source 0 = all 256 tiles, source 1 = south of prev_cut_y
		let progress_pass2_s0 = ps.compute_progress(&order, 0);
		assert!(
			progress_pass2_s0 > progress_end_pass1,
			"progress should increase after processing source 0 in pass 2"
		);

		// After processing source 1 in pass 2: both sources fully done
		let progress_pass2_s1 = ps.compute_progress(&order, 1);
		assert_eq!(
			progress_pass2_s1,
			ps.total_tiles(),
			"all tiles should be completed after pass 2"
		);

		Ok(())
	}
}
