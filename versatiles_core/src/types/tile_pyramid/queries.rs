//! Query methods for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, GeoCenter, TileBBox, TileCover};
use anyhow::Result;

impl TilePyramid {
	/// Returns a reference to the [`TileCover`] at the given zoom level.
	#[must_use]
	pub fn level_ref(&self, level: u8) -> &TileCover {
		&self.levels[level as usize]
	}
	/// Returns a mutable reference to the [`TileCover`] at the given zoom level.
	#[must_use]
	pub fn level_mut(&mut self, level: u8) -> &mut TileCover {
		self.levels.get_mut(level as usize).unwrap()
	}

	/// Returns the bounding box of the given zoom level, or an empty bbox if
	/// the level is empty.
	#[must_use]
	pub fn level_bbox(&self, level: u8) -> TileBBox {
		self.level_ref(level).to_bbox()
	}

	/// Finds the minimum (lowest) non-empty zoom level.
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn level_min(&self) -> Option<u8> {
		self.levels.iter().find(|c| !c.is_empty()).map(TileCover::level)
	}

	/// Finds the maximum (highest) non-empty zoom level.
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn level_max(&self) -> Option<u8> {
		self.levels.iter().rev().find(|c| !c.is_empty()).map(TileCover::level)
	}

	/// Counts the total number of tiles across all zoom levels.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.levels.iter().map(TileCover::count_tiles).sum()
	}

	/// Counts the total number of internal quadtree nodes across all `Tree`
	/// levels.
	#[must_use]
	pub fn count_nodes(&self) -> u64 {
		self
			.levels
			.iter()
			.filter_map(TileCover::as_tree)
			.map(crate::TileQuadtree::count_nodes)
			.sum()
	}

	/// Returns `true` if all levels are empty.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.levels.iter().all(TileCover::is_empty)
	}

	/// Returns an iterator over all zoom-level covers (0 through `MAX_ZOOM_LEVEL`),
	/// including empty ones.
	pub fn iter(&self) -> impl Iterator<Item = &TileCover> + '_ {
		self.levels.iter()
	}

	/// Returns a cloned iterator over all zoom-level covers.
	pub fn to_iter(&self) -> impl Iterator<Item = TileCover> + '_ {
		self.levels.iter().cloned()
	}

	/// Returns an iterator over the bounding boxes of all non-empty zoom levels.
	pub fn to_iter_bboxes(&self) -> impl Iterator<Item = TileBBox> + '_ {
		self.levels.iter().map(TileCover::to_bbox)
	}

	/// Returns a mutable iterator over all zoom-level covers.
	pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut TileCover> + '_ {
		self.levels.iter_mut()
	}

	/// Returns a geographic bounding box covering the union of all non-empty
	/// levels (using the highest non-empty level for maximum precision).
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn geo_bbox(&self) -> Option<GeoBBox> {
		let max_level = self.level_max()?;
		self.levels[max_level as usize].to_geo_bbox()
	}

	/// Calculates a geographic center based on the bounding box at a middle
	/// zoom level.
	///
	/// Returns `None` if the pyramid is empty.
	#[must_use]
	pub fn geo_center(&self) -> Option<GeoCenter> {
		let bbox = self.geo_bbox()?;
		let level = (self.level_min()? + 2).min(self.level_max()?);
		let center_lon = f64::midpoint(bbox.x_min, bbox.x_max);
		let center_lat = f64::midpoint(bbox.y_min, bbox.y_max);
		Some(GeoCenter(center_lon, center_lat, level))
	}

	/// Returns a tile-count-weighted geographic bounding box.
	///
	/// # Errors
	/// Returns an error if the pyramid is empty.
	pub fn weighted_bbox(&self) -> Result<GeoBBox> {
		use anyhow::ensure;
		let mut x_min_sum = 0.0_f64;
		let mut y_min_sum = 0.0_f64;
		let mut x_max_sum = 0.0_f64;
		let mut y_max_sum = 0.0_f64;
		let mut weight_sum = 0.0_f64;
		for cover in &self.levels {
			if let Some(geo) = cover.to_geo_bbox() {
				let weight = cover.count_tiles() as f64;
				x_min_sum += geo.x_min * weight;
				y_min_sum += geo.y_min * weight;
				x_max_sum += geo.x_max * weight;
				y_max_sum += geo.y_max * weight;
				weight_sum += weight;
			}
		}
		ensure!(weight_sum > 0.0, "Cannot compute weighted bbox for an empty pyramid");
		GeoBBox::new(
			x_min_sum / weight_sum,
			y_min_sum / weight_sum,
			x_max_sum / weight_sum,
			y_max_sum / weight_sum,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TileCover, TileQuadtree};

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn count_tiles_and_count_nodes() {
		let mut p = TilePyramid::new_empty();
		p.set_level(TileCover::new_full(2).unwrap()); // 16 tiles, Bbox → 0 tree nodes
		assert_eq!(p.count_tiles(), 16);
		assert_eq!(p.count_nodes(), 0);

		// Insert a tree level
		let qt = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		p.set_level(TileCover::from(qt));
		assert_eq!(p.count_tiles(), 16 + 16);
		// tree has some nodes
	}

	#[test]
	fn get_geo_bbox_and_center() {
		let mut p = TilePyramid::new_empty();
		assert!(p.geo_bbox().is_none());
		assert!(p.geo_center().is_none());

		p.insert_bbox(&bbox(5, 10, 10, 20, 20)).unwrap();
		assert!(p.geo_bbox().is_some());
		assert!(p.geo_center().is_some());
	}

	#[test]
	fn iter_levels_and_iter_all_level_bboxes() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
		p.insert_bbox(&bbox(5, 0, 0, 5, 5)).unwrap();

		assert_eq!(p.iter().filter(|c| !c.is_empty()).count(), 2);
	}

	#[test]
	fn weighted_bbox_empty_errors() {
		assert!(TilePyramid::new_empty().weighted_bbox().is_err());
	}

	#[test]
	fn weighted_bbox_nonempty() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(5, 10, 10, 20, 20)).unwrap();
		assert!(p.weighted_bbox().is_ok());
	}

	#[test]
	fn get_level_bbox_empty_level() {
		let p = TilePyramid::new_empty();
		let b = p.level_bbox(5);
		assert!(b.is_empty());
	}

	// ── level_min / level_max across varying populations ────────────────────
	#[rstest::rstest]
	#[case(&[3], Some(3), Some(3))]
	#[case(&[5, 10], Some(5), Some(10))]
	#[case(&[0, 15, 30], Some(0), Some(30))]
	#[case(&[], None, None)]
	fn level_min_max_from_levels(
		#[case] levels: &[u8],
		#[case] expected_min: Option<u8>,
		#[case] expected_max: Option<u8>,
	) {
		let mut p = TilePyramid::new_empty();
		for l in levels {
			p.insert_bbox(&bbox(*l, 0, 0, 0, 0)).unwrap();
		}
		assert_eq!(p.level_min(), expected_min);
		assert_eq!(p.level_max(), expected_max);
	}

	#[test]
	fn to_iter_bboxes_has_31_entries_and_tracks_population() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(2, 0, 0, 1, 1)).unwrap();
		p.insert_bbox(&bbox(5, 0, 0, 3, 3)).unwrap();
		let bboxes: Vec<_> = p.to_iter_bboxes().collect();
		assert_eq!(bboxes.len(), 31);
		let populated: Vec<u8> = bboxes
			.iter()
			.enumerate()
			.filter(|(_, b)| !b.is_empty())
			.map(|(i, _)| u8::try_from(i).unwrap())
			.collect();
		assert_eq!(populated, vec![2, 5]);
	}

	#[test]
	fn count_tiles_sums_all_levels() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(2, 0, 0, 1, 1)).unwrap(); // 4 tiles
		p.insert_bbox(&bbox(3, 0, 0, 2, 2)).unwrap(); // 9 tiles
		assert_eq!(p.count_tiles(), 13);
	}

	#[test]
	fn weighted_bbox_for_single_level_equals_that_levels_bbox() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(5, 10, 10, 20, 20)).unwrap();
		let wb = p.weighted_bbox().unwrap();
		let gb = p.geo_bbox().unwrap();
		// With only one populated level, the weighted average must equal it.
		assert!((wb.x_min - gb.x_min).abs() < 1e-9);
		assert!((wb.x_max - gb.x_max).abs() < 1e-9);
		assert!((wb.y_min - gb.y_min).abs() < 1e-9);
		assert!((wb.y_max - gb.y_max).abs() < 1e-9);
	}

	#[test]
	fn iter_and_to_iter_match() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
		p.insert_bbox(&bbox(5, 0, 0, 3, 3)).unwrap();
		let a: Vec<_> = p.iter().cloned().collect();
		let b: Vec<_> = p.to_iter().collect();
		assert_eq!(a, b);
	}
}
