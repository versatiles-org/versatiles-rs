use super::*;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord};
use anyhow::Result;
use rstest::rstest;

#[test]
fn test_empty_pyramid() {
	let pyramid = TileBBoxPyramid::new_empty();
	assert!(
		pyramid.is_empty(),
		"Expected new_empty to create an entirely empty pyramid."
	);
	assert_eq!(pyramid.get_level_min(), None);
	assert_eq!(pyramid.get_level_max(), None);
	assert_eq!(pyramid.count_tiles(), 0);
}

#[test]
fn test_full_pyramid() {
	let pyramid = TileBBoxPyramid::new_full_up_to(8);
	assert!(!pyramid.is_empty(), "A 'full' pyramid at level 8 is not empty.");
	// For testing, we expect it to be 'full' up to level 8
	assert!(pyramid.is_full(8));
	// Levels above 8 are empty
	for lvl in 9..MAX_ZOOM_LEVEL {
		assert!(pyramid.get_level_bbox(lvl).is_empty());
	}
}

#[test]
fn test_full_all_levels_pyramid() {
	let pyramid = TileBBoxPyramid::new_full();
	assert!(!pyramid.is_empty(), "A 'full all levels' pyramid is not empty.");
	// All levels should be full
	for lvl in 0..MAX_ZOOM_LEVEL {
		assert!(pyramid.get_level_bbox(lvl).is_full(), "Level {lvl} should be full");
	}
}

#[test]
fn test_intersections() {
	let mut pyramid1 = TileBBoxPyramid::new_empty();
	pyramid1.intersect(&TileBBoxPyramid::new_empty());
	assert!(pyramid1.is_empty());

	let mut pyramid1 = TileBBoxPyramid::new_full_up_to(8);
	pyramid1.intersect(&TileBBoxPyramid::new_empty());
	assert!(pyramid1.is_empty());

	let mut pyramid1 = TileBBoxPyramid::new_empty();
	pyramid1.intersect(&TileBBoxPyramid::new_full_up_to(8));
	assert!(pyramid1.is_empty());

	let mut pyramid1 = TileBBoxPyramid::new_full_up_to(8);
	pyramid1.intersect(&TileBBoxPyramid::new_full_up_to(8));
	assert!(pyramid1.is_full(8));
}

#[test]
fn test_limit_by_geo_bbox() {
	let mut pyramid = TileBBoxPyramid::new_full_up_to(8);
	pyramid
		.intersect_geo_bbox(&GeoBBox::new(8.0653f64, 51.3563f64, 12.3528f64, 52.2564f64).unwrap())
		.unwrap();
	let level_bboxes = pyramid
		.iter_levels()
		.map(std::string::ToString::to_string)
		.collect::<Vec<_>>();
	assert_eq!(
		level_bboxes,
		[
			"0:[0,0,0,0]",
			"1:[1,0,1,0]",
			"2:[2,1,2,1]",
			"3:[4,2,4,2]",
			"4:[8,5,8,5]",
			"5:[16,10,17,10]",
			"6:[33,21,34,21]",
			"7:[66,42,68,42]",
			"8:[133,84,136,85]"
		]
	);
}

#[test]
fn test_include_coord2() -> Result<()> {
	let mut pyramid = TileBBoxPyramid::new_empty();
	pyramid.include_coord(&TileCoord::new(3, 1, 2)?);
	pyramid.include_coord(&TileCoord::new(3, 4, 5)?);
	pyramid.include_coord(&TileCoord::new(8, 6, 7)?);
	let level_bboxes = pyramid
		.iter_levels()
		.map(std::string::ToString::to_string)
		.collect::<Vec<_>>();
	assert_eq!(level_bboxes, ["3:[1,2,4,5]", "8:[6,7,6,7]"]);

	Ok(())
}

#[test]
fn test_include_bbox2() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	pyramid.include_bbox(&TileBBox::from_min_and_max(4, 1, 2, 3, 4).unwrap());
	pyramid.include_bbox(&TileBBox::from_min_and_max(4, 5, 6, 7, 8).unwrap());

	let level_bboxes = pyramid
		.iter_levels()
		.map(std::string::ToString::to_string)
		.collect::<Vec<_>>();
	assert_eq!(level_bboxes, ["4:[1,2,7,8]"]);
}

#[test]
fn test_level_bbox() {
	let test = |level: u8| {
		let mut pyramid = TileBBoxPyramid::new_empty();
		let bbox = TileBBox::new_full(level).unwrap();
		pyramid.set_level_bbox(bbox);
		assert_eq!(pyramid.get_level_bbox(level), &bbox);
	};

	test(0);
	test(1);
	test(29);
	test(30);
}

#[test]
fn test_zoom_min_max2() {
	let test = |z0: u8, z1: u8| {
		let mut pyramid = TileBBoxPyramid::new_full_up_to(z1);
		pyramid.set_level_min(z0);
		assert_eq!(pyramid.get_level_min().unwrap(), z0);
		assert_eq!(pyramid.get_level_max().unwrap(), z1);
	};

	test(0, 1);
	test(0, 30);
	test(30, 30);
}

#[test]
fn test_add_border1() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	pyramid.add_border(1, 2, 3, 4);
	assert!(pyramid.is_empty());

	let mut pyramid = TileBBoxPyramid::new_full_up_to(8);
	pyramid
		.intersect_geo_bbox(&GeoBBox::new(-9., -5., 5., 10.).unwrap())
		.unwrap();
	pyramid.add_border(1, 2, 3, 4);

	let level_bboxes = pyramid
		.iter_levels()
		.map(std::string::ToString::to_string)
		.collect::<Vec<_>>();
	assert_eq!(
		level_bboxes,
		[
			"0:[0,0,0,0]",
			"1:[0,0,1,1]",
			"2:[0,0,3,3]",
			"3:[2,1,7,7]",
			"4:[6,5,11,12]",
			"5:[14,13,19,20]",
			"6:[29,28,35,36]",
			"7:[59,58,68,69]",
			"8:[120,118,134,135]"
		]
	);
}

#[test]
fn test_from_geo_bbox() {
	let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	let pyramid = TileBBoxPyramid::from_geo_bbox(1, 3, &bbox);
	let level_bboxes = pyramid
		.iter_levels()
		.map(std::string::ToString::to_string)
		.collect::<Vec<_>>();
	assert_eq!(level_bboxes, ["1:[0,0,1,1]", "2:[1,1,2,2]", "3:[3,3,4,4]"]);
}

#[test]
fn test_intersect_geo_bbox() {
	let mut pyramid = TileBBoxPyramid::new_full_up_to(5);
	let geo_bbox = GeoBBox::new(-5.0, -2.0, 3.0, 4.0).unwrap();
	pyramid.intersect_geo_bbox(&geo_bbox).unwrap();
	// Now we have a partial coverage at each level up to 5
	assert!(!pyramid.is_empty());
	// We won't check exact tile coords since that depends on the TileBBox logic,
	// but we can check that level 6+ is still empty:
	assert!(pyramid.get_level_bbox(6).is_empty());
}

#[test]
fn test_add_border2() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	// Adding a border to an empty pyramid does nothing
	pyramid.add_border(1, 2, 3, 4);
	assert!(pyramid.is_empty());

	// If we create a partial pyramid and then add a border,
	// each bounding box should expand. We'll rely on the internal tests
	// of `TileBBox` to verify correctness.
	let mut pyramid2 = TileBBoxPyramid::new_full_up_to(3);
	pyramid2.add_border(2, 2, 4, 4);
	// We can't easily test exact numeric outcomes without replicating tile logic,
	// but we can check that it's still not empty.
	assert!(!pyramid2.is_empty());
}

#[test]
fn test_intersect() {
	let mut p1 = TileBBoxPyramid::new_full_up_to(3);
	let p2 = TileBBoxPyramid::new_empty();

	p1.intersect(&p2);
	assert!(
		p1.is_empty(),
		"Intersecting a full pyramid with an empty one yields empty."
	);

	let mut p3 = TileBBoxPyramid::new_full_up_to(3);
	let p4 = TileBBoxPyramid::new_full_up_to(3);
	p3.intersect(&p4);
	assert!(p3.is_full(3), "Full ∩ full = full at the same levels.");
}

#[test]
fn test_get_level_bbox() {
	let pyramid = TileBBoxPyramid::new_full_up_to(2);
	// Level 0, 1, 2 are full, 3 is empty
	assert!(pyramid.get_level_bbox(3).is_empty());
}

#[test]
fn test_set_level_bbox() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	let custom_bbox = TileBBox::new_full(3).unwrap();
	pyramid.set_level_bbox(custom_bbox);
	assert_eq!(pyramid.get_level_bbox(3), &custom_bbox);
}

#[test]
fn test_include_coord1() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	let coord = TileCoord::new(15, 5, 10).unwrap();
	pyramid.include_coord(&coord);
	assert!(!pyramid.get_level_bbox(15).is_empty());
}

#[test]
fn test_include_bbox1() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	let tb = TileBBox::from_min_and_max(6, 10, 10, 12, 12).unwrap();
	pyramid.include_bbox(&tb);
	assert!(!pyramid.get_level_bbox(6).is_empty());
	// No other level should be affected
	assert!(pyramid.get_level_bbox(5).is_empty());
	assert!(pyramid.get_level_bbox(7).is_empty());
}

#[test]
fn test_include_bbox1_pyramid() {
	let mut p1 = TileBBoxPyramid::new_empty();
	let p2 = TileBBoxPyramid::new_full_up_to(2);
	p1.include_pyramid(&p2);
	// Now p1 should have coverage at levels 0..=2
	assert!(p1.get_level_bbox(0).is_full());
	assert!(p1.get_level_bbox(1).is_full());
	assert!(p1.get_level_bbox(2).is_full());
	assert!(p1.get_level_bbox(3).is_empty());
}

#[rstest]
#[case(0, 0, 0, false)]
#[case(10, 100, 199, false)]
#[case(10, 100, 200, true)]
#[case(10, 300, 400, true)]
#[case(10, 300, 401, false)]
#[case(10, 301, 400, false)]
#[case(10, 99, 200, false)]
#[case(11, 300, 400, false)]
fn test_contains_coord(#[case] level: u8, #[case] x: u32, #[case] y: u32, #[case] expected: bool) {
	let mut p = TileBBoxPyramid::new_empty();
	p.include_bbox(&TileBBox::from_min_and_max(10, 100, 200, 300, 400).unwrap());
	assert_eq!(p.includes_coord(&TileCoord::new(level, x, y).unwrap()), expected);
}

#[test]
fn test_overlaps_bbox() {
	let mut p = TileBBoxPyramid::new_empty();
	p.include_bbox(&TileBBox::from_min_and_max(10, 100, 200, 300, 400).unwrap());
	assert!(!p.intersects_bbox(&TileBBox::from_min_and_max(10, 0, 0, 99, 200).unwrap()));
	assert!(!p.intersects_bbox(&TileBBox::from_min_and_max(10, 0, 0, 100, 199).unwrap()));
	assert!(p.intersects_bbox(&TileBBox::from_min_and_max(10, 0, 0, 100, 200).unwrap()));
	assert!(p.intersects_bbox(&TileBBox::from_min_and_max(10, 300, 400, 500, 600).unwrap()));
	assert!(!p.intersects_bbox(&TileBBox::from_min_and_max(10, 300, 401, 500, 600).unwrap()));
	assert!(!p.intersects_bbox(&TileBBox::from_min_and_max(10, 301, 400, 500, 600).unwrap()));
	assert!(!p.intersects_bbox(&TileBBox::from_min_and_max(11, 300, 400, 500, 600).unwrap()));
}

#[test]
fn test_intersects_pyramid() {
	// Two empty pyramids don't intersect
	let p1 = TileBBoxPyramid::new_empty();
	let p2 = TileBBoxPyramid::new_empty();
	assert!(!p1.intersects_pyramid(&p2));

	// Empty and full don't intersect
	assert!(!p1.intersects_pyramid(&TileBBoxPyramid::new_full()));

	// Two full pyramids intersect
	let full1 = TileBBoxPyramid::new_full();
	let full2 = TileBBoxPyramid::new_full();
	assert!(full1.intersects_pyramid(&full2));

	// Non-overlapping at same level
	let mut pa = TileBBoxPyramid::new_empty();
	pa.include_bbox(&TileBBox::from_min_and_max(10, 0, 0, 50, 50).unwrap());
	let mut pb = TileBBoxPyramid::new_empty();
	pb.include_bbox(&TileBBox::from_min_and_max(10, 100, 100, 200, 200).unwrap());
	assert!(!pa.intersects_pyramid(&pb));

	// Overlapping at same level
	let mut pc = TileBBoxPyramid::new_empty();
	pc.include_bbox(&TileBBox::from_min_and_max(10, 40, 40, 150, 150).unwrap());
	assert!(pa.intersects_pyramid(&pc));

	// Different levels only — no intersection
	let mut pd = TileBBoxPyramid::new_empty();
	pd.include_bbox(&TileBBox::from_min_and_max(5, 0, 0, 10, 10).unwrap());
	let mut pe = TileBBoxPyramid::new_empty();
	pe.include_bbox(&TileBBox::from_min_and_max(8, 0, 0, 10, 10).unwrap());
	assert!(!pd.intersects_pyramid(&pe));
}

#[test]
fn test_iter_levels() {
	let p = TileBBoxPyramid::new_full_up_to(2);
	let levels: Vec<u8> = p.iter_levels().map(|tb| tb.level).collect();
	assert_eq!(levels, vec![0, 1, 2]);
}

#[test]
fn test_zoom_min_max1() {
	let p = TileBBoxPyramid::new_full_up_to(3);
	assert_eq!(p.get_level_min(), Some(0));
	assert_eq!(p.get_level_max(), Some(3));

	let empty_p = TileBBoxPyramid::new_empty();
	assert_eq!(empty_p.get_level_min(), None);
	assert_eq!(empty_p.get_level_max(), None);
}

#[test]
fn test_get_good_zoom() {
	let p = TileBBoxPyramid::new_full_up_to(5);
	// Usually, full coverage at level 5 implies many tiles, so we'd find a "good" zoom near 5.
	let good_zoom = p.get_good_level().unwrap();
	// We can't say exactly which level (tile logic is in TileBBox), but typically it'd be 4 or 5
	assert!(good_zoom <= 5);
}

#[test]
fn test_set_zoom_min_max() {
	let mut p = TileBBoxPyramid::new_full_up_to(5);
	// We remove coverage below level 2
	p.set_level_min(2);
	assert_eq!(p.get_level_min(), Some(2));
	assert_eq!(p.get_level_max(), Some(5));

	// Then remove coverage above level 4
	p.set_level_max(4);
	assert_eq!(p.get_level_min(), Some(2));
	assert_eq!(p.get_level_max(), Some(4));
}

#[test]
fn test_count_tiles() {
	let empty_p = TileBBoxPyramid::new_empty();
	assert_eq!(empty_p.count_tiles(), 0);

	// Full coverage typically has many tiles, though exact counts are not trivial
	// without replicating tile coverage logic. We'll just ensure it's not zero.
	let p = TileBBoxPyramid::new_full_up_to(2);
	assert!(p.count_tiles() > 0);
}

#[test]
fn test_get_geo_bbox_and_center() {
	let p = TileBBoxPyramid::new_full_up_to(2);
	// At a basic level, we expect a bounding box covering the globe
	let maybe_bbox = p.get_geo_bbox();
	assert!(maybe_bbox.is_some());
	// The center then should be around (0, 0, some zoom)
	let maybe_center = p.get_geo_center();
	assert!(maybe_center.is_some());
}

#[test]
fn pyramid_swap_xy_transform() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	pyramid.include_bbox(&TileBBox::from_min_and_max(4, 0, 1, 2, 3).unwrap());
	pyramid.swap_xy();
	assert_eq!(
		pyramid.get_level_bbox(4),
		&TileBBox::from_min_and_max(4, 1, 0, 3, 2).unwrap()
	);
}

#[test]
fn pyramid_flip_y_transform() {
	let mut pyramid = TileBBoxPyramid::new_empty();
	pyramid.include_bbox(&TileBBox::from_min_and_max(4, 0, 1, 2, 3).unwrap());
	pyramid.flip_y();
	assert_eq!(
		pyramid.get_level_bbox(4),
		&TileBBox::from_min_and_max(4, 0, 12, 2, 14).unwrap()
	);
}
