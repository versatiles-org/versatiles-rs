use super::translucent_buffer::TranslucentBuffer;
use versatiles_core::{TileBBox, TileBBoxPyramid};

/// Manages multi-pass eviction state for memory-bounded assembly.
///
/// When the translucent tile buffer exceeds `max_buffer_tiles`, northern tiles
/// are evicted and deferred to subsequent passes. `PassState` tracks the
/// horizontal cutline (`cut_y`) and the remaining tile region across passes.
pub struct PassState {
	union_pyramid: TileBBoxPyramid,
	remaining_pyramid: TileBBoxPyramid,
	max_level: u8,
	max_buffer_tiles: usize,
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
