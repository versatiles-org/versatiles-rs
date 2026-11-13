//! Where tiles may be created, kept small enough to go up to level 30.
//!
//! The area at one level is the area of the levels below it, cut to the bounds
//! of the source. Stored that way, level by level, it did not fit in memory
//! (#329): a quadtree cut to an edge that does not lie on a tile edge has a
//! node for every tile along that edge, so each level costs twice the one
//! below it. Two things keep it small here:
//!
//! - The bounds are kept apart, as one rectangle per level, and applied when
//!   the region is asked, instead of being cut into its quadtrees.
//! - Above the source's highest level nothing new is added, so the area has
//!   the same shape at every level from some level on. That level is stored
//!   once, and the levels above it are answered from it.

use anyhow::Result;
use versatiles_core::{GeoBBox, TileBBox, TileCoord, TileCover, TilePyramid};

/// Where a tile may be created, per level from `level_min + 1` to
/// `level_max`.
///
/// More than is created in the end, never less: whether a tile has content is
/// decided by the nearest tile below it, and that one can be empty where a
/// lower one is not.
#[derive(Clone, Debug)]
pub(super) struct FillRegion {
	/// The area per level, not yet cut to the bounds, from `level_min + 1` to
	/// `level_stored`.
	levels: TilePyramid,
	/// The highest level in `levels`. Every level above it has the shape of
	/// this one.
	level_stored: u8,
	/// Per level from 0 to `level_max`, the tiles that reach into the bounds.
	/// The whole level if the source has no bounds.
	reach: Vec<TileBBox>,
	/// Nothing is created at or below this level.
	level_min: u8,
	/// Nothing is created above this level.
	level_max: u8,
}

impl FillRegion {
	/// A region in which nothing is created.
	pub(super) fn empty() -> Self {
		Self {
			levels: TilePyramid::new_empty(),
			level_stored: 0,
			reach: Vec::new(),
			level_min: 0,
			level_max: 0,
		}
	}

	/// A tile can be created wherever the source has a tile at a lower level:
	/// each level takes over the area of the levels below it. The levels in
	/// `content` contribute the area of their *content*, which can be less
	/// than their tiles; the others contribute their tiles. All of it is cut
	/// to the tiles that reach into `bounds`.
	///
	/// # Errors
	/// Returns an error if a cover cannot be built or the bounds cannot be
	/// turned into tiles.
	pub(super) fn new(
		native: &TilePyramid,
		content: &[(u8, TileCover)],
		bounds: Option<&GeoBBox>,
		level_min: u8,
		level_max: u8,
	) -> Result<Self> {
		// From here on, the area stays the same: the source has no tiles above
		// its highest level, and a content cover scaled up beyond its own level
		// keeps its shape.
		let level_steady = content
			.iter()
			.map(|(_, cover)| cover.level())
			.chain([native.level_max().unwrap_or(level_min).saturating_add(1)])
			.max()
			.unwrap_or(level_min);
		let level_stored = level_steady.min(level_max);

		let mut levels = TilePyramid::new_empty();
		// The tiles of the levels below the current one that are not in `content`.
		let lowest = native.level_ref(level_min);
		let mut tiles_below: Option<TileCover> = (!lowest.is_empty()
			&& !content.iter().any(|(content_level, _)| *content_level == level_min))
		.then(|| lowest.clone());

		for level in level_min.saturating_add(1)..=level_stored {
			let mut cover = TileCover::new_empty(level)?;
			for (_, content) in content.iter().filter(|(content_level, _)| *content_level < level) {
				cover = cover.union(&content.at_level(level))?;
			}
			if let Some(tiles_below) = &tiles_below {
				cover = cover.union(&tiles_below.at_level(level))?;
			}
			levels.set_level(cover);

			let tiles = native.level_ref(level);
			if !tiles.is_empty() && !content.iter().any(|(content_level, _)| *content_level == level) {
				tiles_below = Some(match tiles_below {
					Some(below) => below.at_level(level).union(tiles)?,
					None => tiles.clone(),
				});
			}
		}

		let reach = (0..=level_max)
			.map(|level| match bounds {
				Some(bounds) => TileBBox::from_geo_bbox(level, bounds),
				None => TileBBox::new_full(level),
			})
			.collect::<Result<Vec<_>>>()?;

		Ok(Self {
			levels,
			level_stored,
			reach,
			level_min,
			level_max,
		})
	}

	/// The tiles that reach into the bounds at `level`, if tiles are created
	/// there at all.
	fn reach(&self, level: u8) -> Option<&TileBBox> {
		if level <= self.level_min || level > self.level_max {
			return None;
		}
		self.reach.get(usize::from(level))
	}

	/// The stored level that answers for `level`, with its area.
	fn stored(&self, level: u8) -> (u8, &TileCover) {
		let stored = level.min(self.level_stored);
		(stored, self.levels.level_ref(stored))
	}

	/// Whether a tile may be created anywhere in `bbox`.
	pub(super) fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		let Some(reach) = self.reach(bbox.level()) else {
			return false;
		};
		let Ok(inside) = bbox.intersection_bbox(reach) else {
			return false;
		};
		if inside.is_empty() {
			return false;
		}
		let (stored, cover) = self.stored(bbox.level());
		// Exact: above `stored`, the area is `cover` scaled up, so it meets a
		// box if and only if `cover` meets the tiles of `stored` the box lies in.
		cover.intersects_bbox(&inside.at_level(stored))
	}

	/// Whether a tile may be created at `coord`.
	pub(super) fn includes_coord(&self, coord: &TileCoord) -> bool {
		let Some(reach) = self.reach(coord.level) else {
			return false;
		};
		if !reach.includes_coord(coord) {
			return false;
		}
		let (stored, cover) = self.stored(coord.level);
		cover.includes_coord(&coord.at_level(stored))
	}

	/// The area at `level` as a cover, for the pyramid the operation
	/// advertises. Empty outside the levels tiles are created at.
	///
	/// Exact up to the stored levels, and wherever the area takes in all of
	/// the bounds. Above the stored levels, where the bounds cut through the
	/// area, the exact cover would need a node for every tile along the
	/// bounds, so it takes in whole tiles of the highest stored level there:
	/// more tiles than are created, by less than one such tile along the
	/// bounds.
	///
	/// # Errors
	/// Returns an error if a cover cannot be built.
	pub(super) fn cover(&self, level: u8) -> Result<TileCover> {
		let Some(reach) = self.reach(level) else {
			return TileCover::new_empty(level);
		};
		let (stored, cover) = self.stored(level);
		let reach_stored = reach.at_level(stored);
		if cover.includes_bbox(&reach_stored) {
			return Ok(TileCover::from(*reach));
		}
		let cut = if stored == level {
			cover.intersection_bbox(reach)?
		} else {
			cover.intersection_bbox(&reach_stored)?.at_level(level)
		};
		Ok(cut)
	}
}

/// The union of `a` and `b`, kept a rectangle where one of them takes in the
/// other: [`TileCover::union`] always gives a quadtree, and a quadtree of a
/// rectangle whose edges do not lie on tile edges is as large as those edges
/// are long.
///
/// # Errors
/// Returns an error if the covers are at different levels.
pub(super) fn union_of(a: &TileCover, b: TileCover) -> Result<TileCover> {
	if b.is_empty() || a.includes_cover(&b) {
		return Ok(a.clone());
	}
	if a.is_empty() || b.includes_cover(a) {
		return Ok(b);
	}
	a.union(&b)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	/// Bounds whose edges lie on no tile edge at any level.
	fn crooked() -> GeoBBox {
		GeoBBox::new(-180.0, -56.7, 180.0, 83.77).unwrap()
	}

	/// The issue's case: the whole world at level 0, bounds that fall on no
	/// tile edge, and every level up to 30.
	#[test]
	fn thirty_levels_cut_to_crooked_bounds_stay_small() -> Result<()> {
		let native = TilePyramid::new_full_up_to(0);
		let region = FillRegion::new(&native, &[], Some(&crooked()), 0, 30)?;

		assert_eq!(region.level_stored, 1);
		for level in 1..=30 {
			let reach = TileBBox::from_geo_bbox(level, &crooked())?;
			let cover = region.cover(level)?;
			assert_eq!(cover, TileCover::from(reach), "level {level}");
			assert!(
				cover.as_bbox().is_some(),
				"a rectangle, not a quadtree, at level {level}"
			);
		}
		Ok(())
	}

	#[test]
	fn nothing_outside_the_bounds_or_the_levels() -> Result<()> {
		let native = TilePyramid::new_full_up_to(0);
		let region = FillRegion::new(&native, &[], Some(&crooked()), 0, 30)?;

		let reach = TileBBox::from_geo_bbox(30, &crooked())?;
		let (x, y_min, y_max) = (reach.x_min()?, reach.y_min()?, reach.y_max()?);
		assert!(region.includes_coord(&coord(30, x, y_min)));
		assert!(region.includes_coord(&coord(30, x, y_max)));
		assert!(!region.includes_coord(&coord(30, x, y_min - 1)));
		assert!(!region.includes_coord(&coord(30, x, y_max + 1)));
		assert!(!region.includes_coord(&coord(0, 0, 0)), "at the source's lowest level");

		let above = TileBBox::from_min_and_max(30, 0, 0, 100, y_min - 1)?;
		assert!(!region.intersects_bbox(&above));
		let across = TileBBox::from_min_and_max(30, 0, 0, 100, y_min)?;
		assert!(region.intersects_bbox(&across));
		Ok(())
	}

	/// A source with one tile at level 2: above it, the area is that tile,
	/// and only the advertised cover may take in a little more.
	#[test]
	fn above_the_stored_levels_the_shape_stays() -> Result<()> {
		let mut native = TilePyramid::new_empty();
		native.set_level(TileCover::from(TileBBox::from_min_and_max(2, 1, 1, 1, 1)?));
		// Cuts through tile 2/1/1, which spans -90 to 0 and 0 to 66.5, and
		// reaches far east of it.
		let bounds = GeoBBox::new(-80.0, 5.0, 170.0, 60.0)?;
		let region = FillRegion::new(&native, &[], Some(&bounds), 2, 20)?;

		assert_eq!(region.level_stored, 3);
		let inside = coord(2, 1, 1).at_level(20);
		let reach = TileBBox::from_geo_bbox(20, &bounds)?;
		let corner = coord(20, reach.x_min()?, reach.y_min()?);
		assert!(region.includes_coord(&corner));
		assert!(!region.includes_coord(&coord(20, corner.x - 1, corner.y)));
		assert!(
			!region.includes_coord(&coord(20, inside.x + (1 << 18), corner.y)),
			"east of the tile"
		);

		let cover = region.cover(20)?;
		assert!(cover.includes_coord(&corner));
		assert!(
			cover.includes_coord(&inside),
			"less than one tile of level 3 beyond the bounds"
		);
		let tile = TileBBox::from_min_and_max(20, 1 << 18, 1 << 18, (2 << 18) - 1, (2 << 18) - 1)?;
		assert_eq!(cover, TileCover::from(tile));
		Ok(())
	}

	#[test]
	fn a_union_that_one_side_takes_in_stays_a_rectangle() -> Result<()> {
		let full = TileCover::new_full(10)?;
		let part = TileCover::from(TileBBox::from_geo_bbox(10, &crooked())?);
		assert_eq!(union_of(&TileCover::new_empty(10)?, part.clone())?, part);
		assert_eq!(union_of(&part, TileCover::new_empty(10)?)?, part);
		assert!(union_of(&full, part.clone())?.is_full());
		assert!(union_of(&part, full)?.is_full());
		Ok(())
	}
}
