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
