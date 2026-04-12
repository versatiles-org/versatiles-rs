//! Tests for [`TilePyramid`].

use crate::{GeoBBox, MAX_LAT, TileBBox, TileCoord, TileCover, TilePyramid, TileQuadtree};

fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
	TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
}
fn coord(z: u8, x: u32, y: u32) -> TileCoord {
	TileCoord::new(z, x, y).unwrap()
}

// --- constructors ---

#[test]
fn new_empty() {
	let p = TilePyramid::new_empty();
	assert!(p.is_empty());
	assert_eq!(p.get_level_min(), None);
	assert_eq!(p.get_level_max(), None);
	assert_eq!(p.count_tiles(), 0);
}

#[test]
fn new_full() {
	let p = TilePyramid::new_full();
	assert!(!p.is_empty());
	assert_eq!(p.get_level_min(), Some(0));
	assert_eq!(p.get_level_max(), Some(30));
}

#[test]
fn new_full_up_to() {
	let p = TilePyramid::new_full_up_to(5);
	assert_eq!(p.get_level_min(), Some(0));
	assert_eq!(p.get_level_max(), Some(5));
	assert!(p.get_level(6).is_empty());
}

#[test]
fn default_is_empty() {
	assert!(TilePyramid::default().is_empty());
}

#[test]
fn from_geo_bbox() {
	let geo = GeoBBox::new(-180.0, -MAX_LAT, 180.0, MAX_LAT).unwrap();
	let p = TilePyramid::from_geo_bbox(0, 3, &geo).unwrap();
	assert_eq!(p.get_level_min(), Some(0));
	assert_eq!(p.get_level_max(), Some(3));
	assert!(p.get_level(4).is_empty());
}

#[test]
fn from_slice_of_bboxes() {
	let bboxes = vec![bbox(3, 0, 0, 3, 3), bbox(5, 1, 1, 5, 5)];
	let p = TilePyramid::from(bboxes.as_slice());
	assert_eq!(p.get_level_bbox(3), bbox(3, 0, 0, 3, 3));
	assert_eq!(p.get_level_bbox(5), bbox(5, 1, 1, 5, 5));
}

// --- queries ---

#[test]
fn get_level_and_set_level() {
	let mut p = TilePyramid::new_empty();
	let qt = TileQuadtree::new_full(4);
	p.set_level(TileCover::from(qt));
	assert!(!p.get_level(4).is_empty());
	assert!(p.get_level(3).is_empty());
}

#[test]
fn includes_coord() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(5, 3, 4, 10, 15)).unwrap();
	assert!(p.includes_coord(&coord(5, 5, 7)));
	assert!(!p.includes_coord(&coord(5, 0, 0)));
	assert!(!p.includes_coord(&coord(6, 5, 7)));
}

#[test]
fn includes_bbox() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();
	assert!(p.includes_bbox(&bbox(5, 2, 2, 8, 8)).unwrap());
	assert!(!p.includes_bbox(&bbox(5, 0, 0, 20, 20)).unwrap());
}

#[test]
fn intersects_bbox() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(4, 0, 0, 7, 7)).unwrap();
	assert!(p.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
	assert!(!p.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
}

#[test]
fn includes_pyramid_and_intersects_pyramid() {
	let mut a = TilePyramid::new_empty();
	a.include_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();

	let mut b = TilePyramid::new_empty();
	b.include_bbox(&bbox(5, 2, 2, 8, 8)).unwrap();

	assert!(a.includes_pyramid(&b));
	assert!(!b.includes_pyramid(&a));
	assert!(a.intersects_pyramid(&b));

	let mut c = TilePyramid::new_empty();
	c.include_bbox(&bbox(5, 20, 20, 25, 25)).unwrap();
	assert!(!a.intersects_pyramid(&c));
}

#[test]
fn count_tiles_and_count_nodes() {
	let mut p = TilePyramid::new_empty();
	p.set_level(TileCover::new_full(2).unwrap()); // 16 tiles, Bbox → 0 tree nodes
	assert_eq!(p.count_tiles(), 16);
	assert_eq!(p.count_nodes(), 0);

	// Insert a tree level
	let qt = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
	p.set_level(TileCover::from(qt));
	assert_eq!(p.count_tiles(), 16 + 16);
	// tree has some nodes
}

#[test]
fn get_good_level() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(3, 0, 0, 3, 3)).unwrap(); // 16 tiles > 10
	assert_eq!(p.get_good_level(), Some(3));
}

#[test]
fn get_geo_bbox_and_center() {
	let mut p = TilePyramid::new_empty();
	assert!(p.get_geo_bbox().is_none());
	assert!(p.get_geo_center().is_none());

	p.include_bbox(&bbox(5, 10, 10, 20, 20)).unwrap();
	assert!(p.get_geo_bbox().is_some());
	assert!(p.get_geo_center().is_some());
}

#[test]
fn iter_levels_and_iter_all_level_bboxes() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
	p.include_bbox(&bbox(5, 0, 0, 5, 5)).unwrap();

	assert_eq!(p.iter_levels().count(), 2);
	assert_eq!(p.iter_all_level_bboxes().count(), 31); // all 31 levels
}

#[test]
fn intersected_bbox() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(4, 0, 0, 7, 7)).unwrap();
	let result = p.intersected_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
	assert_eq!(result, bbox(4, 4, 4, 7, 7));
}

// --- mutations ---

#[test]
fn include_pyramid() {
	let mut a = TilePyramid::new_empty();
	a.include_bbox(&bbox(5, 0, 0, 5, 5)).unwrap();

	let mut b = TilePyramid::new_empty();
	b.include_bbox(&bbox(5, 10, 10, 15, 15)).unwrap();

	a.include_pyramid(&b);
	assert!(a.includes_coord(&coord(5, 2, 2)));
	assert!(a.includes_coord(&coord(5, 12, 12)));
}

#[test]
fn intersect_pyramid() {
	let mut a = TilePyramid::new_empty();
	a.include_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();

	let mut b = TilePyramid::new_empty();
	b.include_bbox(&bbox(5, 10, 10, 25, 25)).unwrap();

	a.intersect(&b).unwrap();
	assert!(a.includes_coord(&coord(5, 12, 12)));
	assert!(!a.includes_coord(&coord(5, 2, 2)));
}

#[test]
fn intersect_geo_bbox() {
	let mut p = TilePyramid::new_full();
	let geo = GeoBBox::new(10.0, 50.0, 15.0, 55.0).unwrap();
	p.intersect_geo_bbox(&geo).unwrap();
	assert!(!p.is_empty());
	let tiles_at_10 = p.get_level(10).count_tiles();
	assert!(tiles_at_10 > 0);
	assert!(tiles_at_10 < TileQuadtree::new_full(10).count_tiles());
}

#[test]
fn set_level_min_and_max() {
	let mut p = TilePyramid::new_full();
	p.set_level_min(5);
	assert!(p.get_level(4).is_empty());
	assert!(!p.get_level(5).is_empty());

	p.set_level_max(10);
	assert!(!p.get_level(10).is_empty());
	assert!(p.get_level(11).is_empty());
}

#[test]
fn flip_y_and_swap_xy() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
	// Just verify they don't panic
	p.flip_y().unwrap();
	p.swap_xy().unwrap();
	assert!(!p.is_empty());
}

#[test]
fn weighted_bbox_empty_errors() {
	assert!(TilePyramid::new_empty().weighted_bbox().is_err());
}

#[test]
fn weighted_bbox_nonempty() {
	let mut p = TilePyramid::new_empty();
	p.include_bbox(&bbox(5, 10, 10, 20, 20)).unwrap();
	assert!(p.weighted_bbox().is_ok());
}

// --- equality ---

#[test]
fn eq_empty_pyramids() {
	assert_eq!(TilePyramid::new_empty(), TilePyramid::new_empty());
}

#[test]
fn eq_after_same_operations() {
	let mut a = TilePyramid::new_empty();
	a.include_bbox(&bbox(5, 3, 4, 10, 15)).unwrap();

	let mut b = TilePyramid::new_empty();
	b.include_bbox(&bbox(5, 3, 4, 10, 15)).unwrap();

	assert_eq!(a, b);
}
