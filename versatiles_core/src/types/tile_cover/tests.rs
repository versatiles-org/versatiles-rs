//! Tests for [`TileCover`].

use crate::{TileBBox, TileCoord, TileCover, TileQuadtree};

fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
	TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
}
fn coord(z: u8, x: u32, y: u32) -> TileCoord {
	TileCoord::new(z, x, y).unwrap()
}

// --- constructors ---

#[test]
fn new_empty_is_empty() {
	let c = TileCover::new_empty(4).unwrap();
	assert!(c.is_empty());
	assert_eq!(c.level(), 4);
	assert_eq!(c.count_tiles(), 0);
}

#[test]
fn new_full_covers_all() {
	let c = TileCover::new_full(2).unwrap();
	assert!(c.is_full());
	assert_eq!(c.count_tiles(), 16);
}

#[test]
fn from_bbox_and_from_tree() {
	let b = bbox(3, 0, 0, 3, 3);
	let cb = TileCover::from(b);
	assert!(matches!(cb, TileCover::Bbox(_)));
	assert_eq!(cb.count_tiles(), 16);

	let t = TileQuadtree::new_full(3);
	let ct = TileCover::from(t);
	assert!(matches!(ct, TileCover::Tree(_)));
	assert!(ct.is_full());
}

// --- queries ---

#[test]
fn level() {
	assert_eq!(TileCover::new_empty(7).unwrap().level(), 7);
	assert_eq!(TileCover::from(TileQuadtree::new_empty(5)).level(), 5);
}

#[test]
fn bounds_empty_and_nonempty() {
	assert!(TileCover::new_empty(3).unwrap().bounds().is_none());
	let c = TileCover::from(bbox(3, 1, 2, 3, 4));
	assert_eq!(c.bounds(), Some(bbox(3, 1, 2, 3, 4)));
}

#[test]
fn contains_tile() {
	let c = TileCover::from(bbox(5, 3, 4, 10, 15));
	assert!(c.includes_coord(coord(5, 5, 7)).unwrap());
	assert!(!c.includes_coord(coord(5, 0, 0)).unwrap());
}

#[test]
fn contains_bbox() {
	let c = TileCover::from(bbox(5, 0, 0, 15, 15));
	assert!(c.includes_bbox(&bbox(5, 2, 2, 8, 8)).unwrap());
	assert!(!c.includes_bbox(&bbox(5, 0, 0, 16, 16)).unwrap_or(false));
}

#[test]
fn intersects_bbox() {
	let c = TileCover::from(bbox(4, 0, 0, 7, 7));
	assert!(c.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
	assert!(!c.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
}

// --- mutations ---

#[test]
fn insert_tile_expands_bbox() {
	let mut c = TileCover::new_empty(4).unwrap();
	c.include_coord(coord(4, 3, 3)).unwrap();
	assert!(!c.is_empty());
	assert_eq!(c.count_tiles(), 1);
}

#[test]
fn insert_bbox() {
	let mut c = TileCover::new_empty(4).unwrap();
	c.include_bbox(&bbox(4, 2, 2, 5, 5)).unwrap();
	assert_eq!(c.count_tiles(), 16);
}

#[test]
fn remove_tile_upgrades_to_tree() {
	let mut c = TileCover::from(bbox(3, 0, 0, 3, 3)); // 16 tiles
	assert!(matches!(c, TileCover::Bbox(_)));
	c.remove_coord(coord(3, 0, 0)).unwrap();
	assert!(matches!(c, TileCover::Tree(_)));
	assert_eq!(c.count_tiles(), 15);
}

#[test]
fn remove_bbox_upgrades_to_tree() {
	let mut c = TileCover::from(bbox(3, 0, 0, 7, 7)); // full z=3, 64 tiles
	c.remove_bbox(&bbox(3, 0, 0, 3, 3)).unwrap(); // remove 16 tiles
	assert!(matches!(c, TileCover::Tree(_)));
	assert_eq!(c.count_tiles(), 48);
}

// --- set operations ---

#[test]
fn union_bbox_bbox_stays_bbox() {
	let a = TileCover::from(bbox(4, 0, 0, 3, 3));
	let b = TileCover::from(bbox(4, 5, 5, 8, 8));
	let u = a.union(&b).unwrap();
	assert!(matches!(u, TileCover::Bbox(_)));
	// bounding rect of both
	assert_eq!(u.bounds(), Some(bbox(4, 0, 0, 8, 8)));
}

#[test]
fn union_with_tree_gives_tree() {
	let a = TileCover::from(bbox(3, 0, 0, 3, 3));
	let b = TileCover::from(TileQuadtree::new_full(3));
	let u = a.union(&b).unwrap();
	assert!(matches!(u, TileCover::Tree(_)));
	assert!(u.is_full());
}

#[test]
fn intersection_bbox_bbox() {
	let a = TileCover::from(bbox(4, 0, 0, 7, 7));
	let b = TileCover::from(bbox(4, 4, 4, 11, 11));
	let i = a.intersection(&b).unwrap();
	assert!(matches!(i, TileCover::Bbox(_)));
	assert_eq!(i.bounds(), Some(bbox(4, 4, 4, 7, 7)));
}

#[test]
fn difference_always_tree() {
	let a = TileCover::from(bbox(3, 0, 0, 7, 7)); // full z=3, 64 tiles
	let b = TileCover::from(bbox(3, 0, 0, 3, 3)); // 16 tiles
	let d = a.difference(&b).unwrap();
	assert!(matches!(d, TileCover::Tree(_)));
	assert_eq!(d.count_tiles(), 48);
}

// --- conversion ---

#[test]
fn at_level() {
	let c = TileCover::from(bbox(5, 4, 4, 8, 8));
	let c2 = c.at_level(6);
	assert_eq!(c2.level(), 6);
}

#[test]
fn as_bbox_and_as_tree() {
	let cb = TileCover::from(bbox(2, 0, 0, 1, 1));
	assert!(cb.as_bbox().is_some());
	assert!(cb.as_tree().is_none());

	let ct = TileCover::from(TileQuadtree::new_empty(2));
	assert!(ct.as_bbox().is_none());
	assert!(ct.as_tree().is_some());
}

#[test]
fn to_tree_from_bbox() {
	let c = TileCover::from(bbox(3, 1, 1, 4, 4));
	let tree = c.to_tree().unwrap();
	assert_eq!(tree.count_tiles(), 16);
}

// --- equality ---

#[test]
fn eq_bbox_bbox() {
	let a = TileCover::from(bbox(4, 1, 1, 5, 5));
	let b = TileCover::from(bbox(4, 1, 1, 5, 5));
	assert_eq!(a, b);
}

#[test]
fn eq_bbox_tree_same_coverage() {
	let b = bbox(3, 0, 0, 7, 7);
	let cb = TileCover::from(b);
	let ct = TileCover::from(TileQuadtree::from_bbox(&b).unwrap());
	assert_eq!(cb, ct);
}

#[test]
fn neq_different_levels() {
	let a = TileCover::new_empty(2).unwrap();
	let b = TileCover::new_empty(3).unwrap();
	assert_ne!(a, b);
}

// --- to_geo_bbox ---

#[test]
fn to_geo_bbox_empty_is_none() {
	assert!(TileCover::new_empty(4).unwrap().to_geo_bbox().is_none());
}

#[test]
fn to_geo_bbox_nonempty() {
	let c = TileCover::from(bbox(4, 0, 0, 15, 15));
	assert!(c.to_geo_bbox().is_some());
}

// --- is_full for Tree variant ---

#[test]
fn is_full_tree_variant() {
	let c = TileCover::from(TileQuadtree::new_full(3));
	assert!(c.is_full());
	let c2 = TileCover::from(TileQuadtree::new_empty(3));
	assert!(!c2.is_full());
}

// --- intersects_bbox with Tree variant ---

#[test]
fn intersects_bbox_tree_variant() {
	let c = TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7)).unwrap());
	assert!(c.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
	assert!(!c.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
}

// --- level mismatch errors (Tree variant errors; Bbox variant returns Ok(false)) ---

#[test]
fn includes_coord_level_mismatch_tree_errors() {
	// Tree variant errors on zoom mismatch.
	let c = TileCover::from(TileQuadtree::new_full(4));
	assert!(c.includes_coord(coord(5, 0, 0)).is_err());
}

#[test]
fn includes_coord_level_mismatch_bbox_returns_false() {
	// Bbox variant silently returns Ok(false) for level mismatch.
	let c = TileCover::from(bbox(4, 0, 0, 15, 15));
	assert!(!c.includes_coord(coord(5, 0, 0)).unwrap());
}

#[test]
fn includes_bbox_level_mismatch_tree_errors() {
	// Tree variant errors on zoom mismatch.
	let c = TileCover::from(TileQuadtree::new_full(4));
	assert!(c.includes_bbox(&bbox(5, 0, 0, 15, 15)).is_err());
}

#[test]
fn includes_bbox_level_mismatch_bbox_returns_false() {
	// Bbox variant silently returns Ok(false) for level mismatch.
	let c = TileCover::from(bbox(4, 0, 0, 15, 15));
	assert!(!c.includes_bbox(&bbox(5, 0, 0, 15, 15)).unwrap());
}

// --- mutation no-ops ---

#[test]
fn include_coord_noop_when_already_covered() {
	let mut c = TileCover::from(bbox(4, 0, 0, 15, 15));
	// Already covered; stays Bbox and count unchanged.
	c.include_coord(coord(4, 5, 5)).unwrap();
	assert!(matches!(c, TileCover::Bbox(_)));
	assert_eq!(c.count_tiles(), 256);
}

#[test]
fn include_bbox_noop_when_already_covered() {
	let mut c = TileCover::from(bbox(4, 0, 0, 15, 15));
	c.include_bbox(&bbox(4, 2, 2, 8, 8)).unwrap();
	assert!(matches!(c, TileCover::Bbox(_)));
	assert_eq!(c.count_tiles(), 256);
}

#[test]
fn remove_coord_noop_when_not_in_bbox() {
	let mut c = TileCover::from(bbox(4, 5, 5, 10, 10));
	// coord outside bbox → no-op, stays Bbox
	c.remove_coord(coord(4, 0, 0)).unwrap();
	assert!(matches!(c, TileCover::Bbox(_)));
}

#[test]
fn remove_bbox_noop_when_no_overlap() {
	let mut c = TileCover::from(bbox(4, 5, 5, 10, 10));
	// non-overlapping bbox → no-op, stays Bbox
	c.remove_bbox(&bbox(4, 12, 12, 15, 15)).unwrap();
	assert!(matches!(c, TileCover::Bbox(_)));
}

// --- set-ops zoom-mismatch errors ---

#[test]
fn set_ops_zoom_mismatch_errors() {
	let a = TileCover::from(bbox(3, 0, 0, 7, 7));
	let b = TileCover::from(bbox(4, 0, 0, 15, 15));
	assert!(a.union(&b).is_err());
	assert!(a.intersection(&b).is_err());
	assert!(a.difference(&b).is_err());
}

// --- Display / Debug ---

#[test]
fn display_bbox_variant() {
	let c = TileCover::from(bbox(3, 0, 0, 7, 7));
	let s = format!("{c}");
	assert!(!s.is_empty());
}

#[test]
fn display_tree_variant() {
	let c = TileCover::from(TileQuadtree::new_full(3));
	let s = format!("{c}");
	assert!(s.contains("zoom=3"));
}

// --- iteration ---

#[test]
fn iter_tiles_count() {
	let c = TileCover::from(bbox(3, 0, 0, 3, 3));
	assert_eq!(c.iter_coords().count(), 16);
}

#[test]
fn iter_bbox_grid_empty() {
	let c = TileCover::new_empty(4).unwrap();
	assert_eq!(c.iter_bbox_grid(4).count(), 0);
}

#[test]
fn iter_bbox_grid_nonempty() {
	let c = TileCover::from(bbox(4, 0, 0, 7, 7));
	// 8×8 tiles split into 4×4 blocks → 4 blocks
	assert_eq!(c.iter_bbox_grid(4).count(), 4);
}
