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
		self.levels.get_mut(level as usize).expect("level <= MAX_ZOOM_LEVEL")
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
	use rstest::rstest;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	fn pyramid_from_bboxes(bs: &[TileBBox]) -> TilePyramid {
		let mut p = TilePyramid::new_empty();
		for b in bs {
			p.insert_bbox(b).unwrap();
		}
		p
	}

	/// `count_tiles` across different level storage kinds. `count_nodes`
	/// counts internal quadtree nodes, which Bbox covers skip entirely (0)
	/// but Tree covers contribute to.
	#[rstest]
	#[case::bbox_only_is_node_free(
		{
			let mut p = TilePyramid::new_empty();
			p.set_level(TileCover::new_full(2).unwrap());
			p
		},
		16,
		0,
	)]
	#[case::bbox_plus_tree_has_tree_nodes(
		{
			let mut p = TilePyramid::new_empty();
			p.set_level(TileCover::new_full(2).unwrap());
			p.set_level(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3))));
			p
		},
		16 + 16,
		5, // empirically: a 4×4 block at z=3 yields 5 tree nodes (1 partial + 4 children, after normalisation).
	)]
	#[case::two_bboxes(pyramid_from_bboxes(&[bbox(2, 0, 0, 1, 1), bbox(3, 0, 0, 2, 2)]), 13, 0)]
	fn count_cases(#[case] p: TilePyramid, #[case] expected_tiles: u64, #[case] expected_nodes: u64) {
		assert_eq!(p.count_tiles(), expected_tiles);
		assert_eq!(p.count_nodes(), expected_nodes);
	}

	/// Empty pyramid → geo_bbox/geo_center None; populated → Some.
	#[rstest]
	#[case::empty(TilePyramid::new_empty(), false)]
	#[case::populated(pyramid_from_bboxes(&[bbox(5, 10, 10, 20, 20)]), true)]
	fn geo_bbox_and_center_are_some_when_populated(#[case] p: TilePyramid, #[case] expect_some: bool) {
		assert_eq!(p.geo_bbox().is_some(), expect_some);
		assert_eq!(p.geo_center().is_some(), expect_some);
	}

	#[test]
	fn iter_skips_empty_levels() {
		let p = pyramid_from_bboxes(&[bbox(3, 0, 0, 3, 3), bbox(5, 0, 0, 5, 5)]);
		assert_eq!(p.iter().filter(|c| !c.is_empty()).count(), 2);
	}

	/// `weighted_bbox` — empty errors, single populated level returns that
	/// level's bbox exactly.
	#[rstest]
	#[case::empty_errors(TilePyramid::new_empty(), None)]
	#[case::single_level(pyramid_from_bboxes(&[bbox(5, 10, 10, 20, 20)]), Some(()))]
	fn weighted_bbox_cases(#[case] p: TilePyramid, #[case] expect: Option<()>) {
		match (p.weighted_bbox(), expect) {
			(Ok(wb), Some(())) => {
				// With only one populated level, the weighted average must match it.
				let gb = p.geo_bbox().unwrap();
				assert!((wb.x_min - gb.x_min).abs() < 1e-9);
				assert!((wb.x_max - gb.x_max).abs() < 1e-9);
				assert!((wb.y_min - gb.y_min).abs() < 1e-9);
				assert!((wb.y_max - gb.y_max).abs() < 1e-9);
			}
			(Err(_), None) => {}
			(got, want) => panic!("weighted_bbox mismatch: got={got:?}, expected some={}", want.is_some()),
		}
	}

	#[test]
	fn get_level_bbox_empty_level() {
		assert!(TilePyramid::new_empty().level_bbox(5).is_empty());
	}

	// ── level_min / level_max across varying populations ────────────────────
	#[rstest]
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
		let p = pyramid_from_bboxes(&[bbox(2, 0, 0, 1, 1), bbox(5, 0, 0, 3, 3)]);
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
	fn iter_and_to_iter_yield_same_sequence() {
		let p = pyramid_from_bboxes(&[bbox(3, 0, 0, 3, 3), bbox(5, 0, 0, 3, 3)]);
		let a: Vec<_> = p.iter().cloned().collect();
		let b: Vec<_> = p.to_iter().collect();
		assert_eq!(a, b);
	}
}
