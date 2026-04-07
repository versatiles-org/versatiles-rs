use crate::{TileBBox, TileBBoxPyramid, TileCoord};
use anyhow::Result;

impl TileBBoxPyramid {
	/// Returns a reference to the bounding box at the specified zoom level.
	///
	/// # Panics
	///
	/// Panics if `level` >= `MAX_ZOOM_LEVEL`.
	#[must_use]
	pub fn get_level_bbox(&self, level: u8) -> &TileBBox {
		&self.level_bbox[level as usize]
	}

	/// Checks if the pyramid contains the given `(x, y, z)` tile coordinate.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		if let Some(bbox) = self.level_bbox.get(coord.level as usize) {
			bbox.includes_coord(coord)
		} else {
			false
		}
	}

	/// Checks if the pyramid completely includes the specified bounding box at the bounding box's zoom level.
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		if let Some(local_bbox) = self.level_bbox.get(bbox.level as usize) {
			local_bbox.includes_bbox(bbox)
		} else {
			false
		}
	}

	/// Checks if this pyramid completely includes another pyramid at all zoom levels.
	#[must_use]
	pub fn includes_pyramid(&self, other: &TileBBoxPyramid) -> bool {
		for bbox_other in other.iter_levels() {
			let bbox_self = self.get_level_bbox(bbox_other.level);
			if !bbox_self.includes_bbox(bbox_other) {
				return false;
			}
		}
		true
	}

	/// Checks if the pyramid overlaps the specified bounding box at the bounding box's zoom level.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		if let Some(local_bbox) = self.level_bbox.get(bbox.level as usize) {
			local_bbox.intersects_bbox(bbox)
		} else {
			false
		}
	}

	/// Checks if this pyramid intersects (overlaps) another pyramid at any level.
	#[must_use]
	pub fn intersects_pyramid(&self, other: &TileBBoxPyramid) -> bool {
		for bbox1 in self.iter_levels() {
			let bbox2 = other.get_level_bbox(bbox1.level);
			if bbox1.intersects_bbox(bbox2) {
				return true;
			}
		}
		false
	}

	/// Returns an iterator over all **non-empty** bounding boxes in this pyramid.
	pub fn iter_levels(&self) -> impl Iterator<Item = &TileBBox> {
		self.level_bbox.iter().filter(|bbox| !bbox.is_empty())
	}

	/// Finds the minimum zoom level that contains any tiles.
	///
	/// Returns `None` if **all** levels are empty.
	#[must_use]
	pub fn get_level_min(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.find(|bbox| !bbox.is_empty())
			.map(|bbox| bbox.level)
	}

	/// Finds the maximum zoom level that contains any tiles.
	///
	/// Returns `None` if **all** levels are empty.
	#[must_use]
	pub fn get_level_max(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.rev()
			.find(|bbox| !bbox.is_empty())
			.map(|bbox| bbox.level)
	}

	/// Returns a "good" zoom level, heuristically one that has more than 10 tiles.
	///
	/// This scans from the highest zoom level downward, returning the first that meets
	/// a threshold of `> 10` tiles. Returns `None` if none meet that threshold.
	#[must_use]
	pub fn get_good_level(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.rev()
			.find(|bbox| bbox.count_tiles() > 10)
			.map(|bbox| bbox.level)
	}

	pub fn intersected_bbox(&self, bbox: &TileBBox) -> Result<TileBBox> {
		if let Some(level_bbox) = self.level_bbox.get(bbox.level as usize) {
			level_bbox.intersected_bbox(bbox)
		} else {
			TileBBox::new_empty(bbox.level)
		}
	}

	/// Counts the total number of tiles across all non-empty bounding boxes in this pyramid.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.level_bbox.iter().map(TileBBox::count_tiles).sum()
	}

	/// Checks if **all** bounding boxes in this pyramid are empty.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.level_bbox.iter().all(TileBBox::is_empty)
	}

	/// Checks if this pyramid is "full" up to the specified zoom level, meaning
	/// each relevant bounding box is flagged as full coverage.
	#[cfg(test)]
	#[must_use]
	pub fn is_full(&self, max_zoom_level: u8) -> bool {
		self.level_bbox.iter().all(|bbox| {
			if bbox.level <= max_zoom_level {
				bbox.is_full()
			} else {
				bbox.is_empty()
			}
		})
	}
}
