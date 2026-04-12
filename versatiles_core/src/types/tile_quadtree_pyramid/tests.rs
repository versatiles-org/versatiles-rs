//! Tests for [`TileQuadtreePyramid`].

use crate::{GeoBBox, MAX_LAT, TileBBox, TileBBoxPyramid, TileCoord, TileQuadtree, TileQuadtreePyramid};
use rstest::rstest;

fn make_bbox(zoom: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> TileBBox {
	TileBBox::from_min_and_max(zoom, x_min, y_min, x_max, y_max).unwrap()
}

#[test]
fn test_new_empty() {
	let pyramid = TileQuadtreePyramid::new_empty();
	assert!(pyramid.is_empty());
	assert_eq!(pyramid.get_level_min(), None);
	assert_eq!(pyramid.get_level_max(), None);
	assert_eq!(pyramid.tile_count(), 0);
	assert_eq!(pyramid.get_geo_bbox(), None);
}

#[test]
fn test_new_full() {
	let pyramid = TileQuadtreePyramid::new_full();
	assert!(!pyramid.is_empty());
	assert_eq!(pyramid.get_level_min(), Some(0));
	assert_eq!(pyramid.get_level_max(), Some(30));
	// At zoom 0 there is 1 tile; total is sum of 4^z for z in 0..=30
	let expected: u64 = (0u32..=30).map(|z| 4u64.pow(z)).sum();
	assert_eq!(pyramid.tile_count(), expected);
}

#[test]
fn test_from_bbox_pyramid_roundtrip() {
	// Build a TileBBoxPyramid with specific bounds, convert to quadtree pyramid,
	// convert back, and verify the bounds match.
	let mut src = TileBBoxPyramid::new_empty();
	src.set_level_bbox(make_bbox(5, 3, 4, 10, 15));
	src.set_level_bbox(make_bbox(8, 50, 60, 100, 120));

	let qt_pyramid = TileQuadtreePyramid::from_bbox_pyramid(&src).unwrap();
	let result = qt_pyramid.to_bbox_pyramid();

	assert_eq!(result.get_level_bbox(5), src.get_level_bbox(5));
	assert_eq!(result.get_level_bbox(8), src.get_level_bbox(8));
	// Levels that were empty should remain empty
	assert!(result.get_level_bbox(0).is_empty());
	assert!(result.get_level_bbox(10).is_empty());
}

#[test]
fn test_include_bbox_and_includes_coord() {
	let mut pyramid = TileQuadtreePyramid::new_empty();
	let b = make_bbox(5, 3, 4, 5, 6);
	pyramid.include_bbox(&b).unwrap();

	// A coord inside the bbox
	let inside = TileCoord::new(5, 4, 5).unwrap();
	assert!(pyramid.includes_coord(&inside));

	// A coord outside the bbox
	let outside = TileCoord::new(5, 10, 10).unwrap();
	assert!(!pyramid.includes_coord(&outside));

	// A coord at a different zoom level
	let other_zoom = TileCoord::new(6, 4, 5).unwrap();
	assert!(!pyramid.includes_coord(&other_zoom));
}

#[test]
fn test_includes_bbox() {
	let mut pyramid = TileQuadtreePyramid::new_empty();
	let full_region = make_bbox(5, 0, 0, 31, 31);
	pyramid.include_bbox(&full_region).unwrap();

	// The full region should be included
	assert!(pyramid.includes_bbox(&full_region).unwrap());

	// A sub-region should be included
	let sub = make_bbox(5, 2, 2, 10, 10);
	assert!(pyramid.includes_bbox(&sub).unwrap());

	// The full tile space is different from the covered sub-region
	// Check that tiles NOT in the covered sub-region (0,0 to 31,31) but in a
	// non-overlapping part of tile space are not included.
	// Actually, our covered region IS 0,0 to 31,31 which is the full zoom-5 space.
	// Use a different bbox that is NOT fully covered to test false case.
	// Include only a small sub-region and check something outside it.
	let mut pyramid2 = TileQuadtreePyramid::new_empty();
	pyramid2.include_bbox(&make_bbox(5, 0, 0, 5, 5)).unwrap();
	let outside2 = make_bbox(5, 10, 10, 20, 20);
	assert!(!pyramid2.includes_bbox(&outside2).unwrap());
}

#[test]
fn test_intersect_geo_bbox() {
	// Start with a full pyramid and intersect with a small geo region
	let mut pyramid = TileQuadtreePyramid::new_full();
	let geo_bbox = GeoBBox::new(10.0, 50.0, 15.0, 55.0).unwrap();
	pyramid.intersect_geo_bbox(&geo_bbox).unwrap();

	// Should no longer be empty
	assert!(!pyramid.is_empty());

	// At zoom 0 the whole world is one tile, still covered
	assert!(!pyramid.get_level(0).is_empty());

	// At higher zoom levels only a small area should be covered
	let tiles_at_10 = pyramid.get_level(10).tile_count();
	let full_at_10 = TileQuadtree::new_full(10).tile_count();
	assert!(tiles_at_10 < full_at_10);
	assert!(tiles_at_10 > 0);
}

#[test]
fn test_set_zoom_min() {
	let mut pyramid = TileQuadtreePyramid::new_full();
	pyramid.set_zoom_min(5);

	// Levels 0-4 should be empty
	for z in 0..5u8 {
		assert!(pyramid.get_level(z).is_empty(), "Expected level {z} to be empty");
	}
	// Level 5 and above should remain full
	assert!(pyramid.get_level(5).is_full());
	assert!(pyramid.get_level(10).is_full());
	assert_eq!(pyramid.get_level_min(), Some(5));
}

#[test]
fn test_set_zoom_max() {
	let mut pyramid = TileQuadtreePyramid::new_full();
	pyramid.set_zoom_max(5);

	// Levels above 5 should be empty
	for z in 6..=30u8 {
		assert!(pyramid.get_level(z).is_empty(), "Expected level {z} to be empty");
	}
	// Levels 0-5 should remain full
	assert!(pyramid.get_level(0).is_full());
	assert!(pyramid.get_level(5).is_full());
	assert_eq!(pyramid.get_level_max(), Some(5));
}

#[test]
fn test_count_tiles() {
	let mut pyramid = TileQuadtreePyramid::new_empty();
	// At zoom 2: 4^2 = 16 tiles total when full
	pyramid.set_level(TileQuadtree::new_full(2));
	assert_eq!(pyramid.tile_count(), 16);

	// Add zoom 3: 4^3 = 64 tiles
	pyramid.set_level(TileQuadtree::new_full(3));
	assert_eq!(pyramid.tile_count(), 80);
}

#[test]
fn test_get_geo_bbox() {
	let mut pyramid = TileQuadtreePyramid::new_empty();
	assert_eq!(pyramid.get_geo_bbox(), None);

	let b = make_bbox(5, 10, 10, 20, 20);
	pyramid.include_bbox(&b).unwrap();

	let geo = pyramid.get_geo_bbox();
	assert!(geo.is_some(), "Expected a geo bbox after including tiles");
}

#[test]
fn test_from_geo_bbox() {
	let geo_bbox = GeoBBox::new(-180.0, -85.0, 180.0, 85.0).unwrap();
	let pyramid = TileQuadtreePyramid::from_geo_bbox(0, 5, &geo_bbox).unwrap();

	// Levels 0-5 should be non-empty
	for z in 0..=5u8 {
		assert!(!pyramid.get_level(z).is_empty(), "Expected level {z} to be non-empty");
	}
	// Levels above 5 should be empty
	for z in 6..=30u8 {
		assert!(pyramid.get_level(z).is_empty(), "Expected level {z} to be empty");
	}

	assert_eq!(pyramid.get_level_min(), Some(0));
	assert_eq!(pyramid.get_level_max(), Some(5));
}

#[test]
fn test_include_pyramid() {
	let mut a = TileQuadtreePyramid::new_empty();
	a.include_bbox(&make_bbox(5, 0, 0, 5, 5)).unwrap();

	let mut b = TileQuadtreePyramid::new_empty();
	b.include_bbox(&make_bbox(5, 10, 10, 15, 15)).unwrap();

	a.include_pyramid(&b);

	// Both regions should now be in a
	let inside_a = TileCoord::new(5, 2, 2).unwrap();
	let inside_b = TileCoord::new(5, 12, 12).unwrap();
	assert!(a.includes_coord(&inside_a));
	assert!(a.includes_coord(&inside_b));
}

#[test]
fn test_intersect() {
	let mut a = TileQuadtreePyramid::new_empty();
	a.include_bbox(&make_bbox(5, 0, 0, 15, 15)).unwrap();

	let mut b = TileQuadtreePyramid::new_empty();
	b.include_bbox(&make_bbox(5, 10, 10, 25, 25)).unwrap();

	a.intersect(&b).unwrap();

	// Only the overlapping region 10-15 should remain
	let in_overlap = TileCoord::new(5, 12, 12).unwrap();
	let only_a = TileCoord::new(5, 2, 2).unwrap();
	let only_b = TileCoord::new(5, 22, 22).unwrap();

	assert!(a.includes_coord(&in_overlap));
	assert!(!a.includes_coord(&only_a));
	assert!(!a.includes_coord(&only_b));
}

#[test]
fn test_default_is_empty() {
	let pyramid = TileQuadtreePyramid::default();
	assert!(pyramid.is_empty());
}

#[test]
fn test_set_level() {
	let mut pyramid = TileQuadtreePyramid::new_empty();
	let qt = TileQuadtree::new_full(7);
	pyramid.set_level(qt);
	assert!(pyramid.get_level(7).is_full());
	assert!(pyramid.get_level(6).is_empty());
}

// ---------------------------------------------------------------------------
// Parametrized tile-count and node-count checks for geo-bbox pyramids
//
// Cases are (geo_bbox as (x_min,y_min,x_max,y_max), max_zoom, expected_tiles, expected_nodes).
//
// "world"   = full Web-Mercator extent → every level is Full → 0 partial nodes.
// "germany" = (5.9, 47.3, 15.0, 55.1), small Central-European box.
// "europe"  = (-10.0, 35.0, 40.0, 72.0), wider European box.
// ---------------------------------------------------------------------------
#[rstest]
// world: every zoom level is a Full node, so nodes always 0
//   tile total = sum(4^z for z in 0..=max_zoom)
#[case::global_0((-180.0, -MAX_LAT, 180.0, MAX_LAT), 0, 31,    1)]
#[case::global_1((-180.0, -MAX_LAT, 180.0, MAX_LAT), 1, 31,    5)]
#[case::global_2((-180.0, -MAX_LAT, 180.0, MAX_LAT), 2, 31,   21)]
#[case::global_3((-180.0, -MAX_LAT, 180.0, MAX_LAT), 3, 31,   85)]
#[case::global_4((-180.0, -MAX_LAT, 180.0, MAX_LAT), 4, 31, 341)]
#[case::global_5((-180.0, -MAX_LAT, 180.0, MAX_LAT), 5, 31, 1365)]
#[case::global_10((-180.0, -MAX_LAT, 180.0, MAX_LAT), 10, 31, 1398101)]
#[case::global_20((-180.0, -MAX_LAT, 180.0, MAX_LAT), 20, 31, 1466015503701)]
#[case::global_30((-180.0, -MAX_LAT, 180.0, MAX_LAT), 30, 31, 1537228672809129301)]
// europe: more complex shape, so we get a mix of Full, Empty, and Partial nodes → nonzero node count
#[case::europe_0((-10.0, 35.0, 40.0, 72.0), 0, 31,  1)]
#[case::europe_1((-10.0, 35.0, 40.0, 72.0), 1, 35,  3)]
#[case::europe_2((-10.0, 35.0, 40.0, 72.0), 2, 47,  7)]
#[case::europe_4((-10.0, 35.0, 40.0, 72.0), 4, 123, 25)]
#[case::europe_8((-10.0, 35.0, 40.0, 72.0), 8, 875, 2515)]
#[case::europe_16((-10.0, 35.0, 40.0, 72.0), 16, 272699, 150667447)]
// berlin: smaller box, so fewer tiles and nodes than europe
#[case::berlin_0((13.1, 52.3, 13.8, 52.7), 0, 31, 1)]
#[case::berlin_1((13.1, 52.3, 13.8, 52.7), 1, 35, 2)]
#[case::berlin_2((13.1, 52.3, 13.8, 52.7), 2, 43, 3)]
#[case::berlin_3((13.1, 52.3, 13.8, 52.7), 3, 55, 4)]
#[case::berlin_4((13.1, 52.3, 13.8, 52.7), 4, 71, 5)]
#[case::berlin_8((13.1, 52.3, 13.8, 52.7), 8, 187, 12)]
#[case::berlin_10((13.1, 52.3, 13.8, 52.7), 10, 295, 25)]
#[case::berlin_20((13.1, 52.3, 13.8, 52.7), 20, 42771, 5211256)]
fn test_geo_bbox_tile_and_node_counts(
	#[case] bbox: (f64, f64, f64, f64),
	#[case] max_zoom: u8,
	#[case] expected_nodes: u64,
	#[case] expected_tiles: u64,
) {
	let (x_min, y_min, x_max, y_max) = bbox;
	let geo_bbox = GeoBBox::new(x_min, y_min, x_max, y_max).unwrap();
	let pyramid = TileQuadtreePyramid::from_geo_bbox(0, max_zoom, &geo_bbox).unwrap();
	assert_eq!(pyramid.node_count(), expected_nodes);
	assert_eq!(pyramid.tile_count(), expected_tiles);
}
