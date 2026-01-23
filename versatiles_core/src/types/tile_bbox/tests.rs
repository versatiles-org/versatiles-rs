#![allow(clippy::cast_possible_truncation)]

use crate::{GeoBBox, TileBBox, TileBBoxPyramid, TileCoord};
use anyhow::Result;
use rstest::rstest;

#[rstest]
#[case((4, 5, 12, 5, 12), 1)]
#[case((4, 5, 12, 7, 15), 12)]
#[case((4, 5, 12, 5, 15), 4)]
#[case((4, 5, 15, 7, 15), 3)]
fn count_tiles_cases(#[case] args: (u8, u32, u32, u32, u32), #[case] expected: u64) {
	let (l, x0, y0, x1, y1) = args;
	assert_eq!(
		TileBBox::from_min_and_max(l, x0, y0, x1, y1).unwrap().count_tiles(),
		expected
	);
}

#[test]
fn from_geo() {
	let bbox1 = TileBBox::from_geo(9, &GeoBBox::new(8.0653, 51.3563, 12.3528, 52.2564).unwrap()).unwrap();
	let bbox2 = TileBBox::from_min_and_max(9, 267, 168, 273, 170).unwrap();
	assert_eq!(bbox1, bbox2);
}

#[test]
fn from_geo_is_not_empty() {
	let bbox1 = TileBBox::from_geo(0, &GeoBBox::new(8.0, 51.0, 8.000001f64, 51.0).unwrap()).unwrap();
	assert_eq!(bbox1.count_tiles(), 1);
	assert!(!bbox1.is_empty());

	let bbox2 = TileBBox::from_geo(14, &GeoBBox::new(-132.000001, -40.0, -132.0, -40.0).unwrap()).unwrap();
	assert_eq!(bbox2.count_tiles(), 1);
	assert!(!bbox2.is_empty());
}

#[test]
fn quarter_planet() {
	let geo_bbox = GeoBBox::new(0.0, -85.05112877980659f64, 180.0, 0.0).unwrap();
	for level in 1..30 {
		let bbox = TileBBox::from_geo(level, &geo_bbox).unwrap();
		assert_eq!(bbox.count_tiles(), 4u64.pow(u32::from(level) - 1));
		assert_eq!(bbox.to_geo_bbox().unwrap(), geo_bbox);
	}
}

#[test]
fn sa_pacific() {
	let geo_bbox = GeoBBox::new(-180.0, -66.51326044311186f64, -90.0, 0.0).unwrap();
	for level in 2..30 {
		let bbox = TileBBox::from_geo(level, &geo_bbox).unwrap();
		assert_eq!(bbox.count_tiles(), 4u64.pow(u32::from(level) - 2));
		assert_eq!(bbox.to_geo_bbox().unwrap(), geo_bbox);
	}
}

#[test]
fn boolean_operations() {
	/*
		  #---#
	  #---# |
	  | | | |
	  | #-|-#
	  #---#
	*/
	let bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13).unwrap();
	let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12).unwrap();

	let mut bbox1_intersect = bbox1;
	bbox1_intersect.intersect_with(&bbox2).unwrap();
	assert_eq!(bbox1_intersect, TileBBox::from_min_and_max(4, 1, 11, 2, 12).unwrap());

	let mut bbox1_union = bbox1;
	bbox1_union.include_bbox(&bbox2).unwrap();
	assert_eq!(bbox1_union, TileBBox::from_min_and_max(4, 0, 10, 3, 13).unwrap());
}

#[test]
fn include_tile() {
	let mut bbox = TileBBox::from_min_and_max(4, 0, 1, 2, 3).unwrap();
	bbox.include(4, 5);
	assert_eq!(bbox, TileBBox::from_min_and_max(4, 0, 1, 4, 5).unwrap());
}

#[test]
fn empty_or_full() {
	let mut bbox1 = TileBBox::new_empty(12).unwrap();
	assert!(bbox1.is_empty());

	bbox1.set_full();
	assert!(bbox1.is_full());

	let mut bbox1 = TileBBox::new_full(13).unwrap();
	assert!(bbox1.is_full());

	bbox1.set_empty();
	assert!(bbox1.is_empty());
}

#[test]
fn iter_coords() {
	let bbox = TileBBox::from_min_and_max(16, 1, 5, 2, 6).unwrap();
	let vec: Vec<TileCoord> = bbox.iter_coords().collect();
	assert_eq!(vec.len(), 4);
	assert_eq!(vec[0], TileCoord::new(16, 1, 5).unwrap());
	assert_eq!(vec[1], TileCoord::new(16, 2, 5).unwrap());
	assert_eq!(vec[2], TileCoord::new(16, 1, 6).unwrap());
	assert_eq!(vec[3], TileCoord::new(16, 2, 6).unwrap());
}

#[rstest]
#[case(16, (10, 0, 0, 31, 31), "0,0,15,15 16,0,31,15 0,16,15,31 16,16,31,31")]
#[case(16, (10, 5, 6, 25, 26), "5,6,15,15 16,6,25,15 5,16,15,26 16,16,25,26")]
#[case(16, (10, 5, 6, 16, 16), "5,6,15,15 16,6,16,15 5,16,15,16 16,16,16,16")]
#[case(16, (10, 5, 6, 16, 15), "5,6,15,15 16,6,16,15")]
#[case(16, (10, 6, 7, 6, 7), "6,7,6,7")]
#[case(64, (4, 6, 7, 6, 7), "6,7,6,7")]
fn iter_bbox_grid_cases(#[case] size: u32, #[case] def: (u8, u32, u32, u32, u32), #[case] expected: &str) {
	let bbox = TileBBox::from_min_and_max(def.0, def.1, def.2, def.3, def.4).unwrap();
	let result: String = bbox
		.iter_bbox_grid(size)
		.map(|bbox| {
			format!(
				"{},{},{},{}",
				bbox.x_min().unwrap(),
				bbox.y_min().unwrap(),
				bbox.x_max().unwrap(),
				bbox.y_max().unwrap()
			)
		})
		.collect::<Vec<String>>()
		.join(" ");
	assert_eq!(result, expected);
}

#[test]
fn add_border() {
	let mut bbox = TileBBox::from_min_and_max(8, 5, 10, 20, 30).unwrap();

	// Border of (1, 1, 1, 1) should increase the size of the bbox by 1 in all directions
	bbox.expand_by(1, 1, 1, 1);
	assert_eq!(bbox, TileBBox::from_min_and_max(8, 4, 9, 21, 31).unwrap());

	// Border of (2, 3, 4, 5) should further increase the size of the bbox
	bbox.expand_by(2, 3, 4, 5);
	assert_eq!(bbox, TileBBox::from_min_and_max(8, 2, 6, 25, 36).unwrap());

	// Border of (0, 0, 0, 0) should not change the size of the bbox
	bbox.expand_by(0, 0, 0, 0);
	assert_eq!(bbox, TileBBox::from_min_and_max(8, 2, 6, 25, 36).unwrap());

	// Large border should saturate at max=255 for level=8
	bbox.expand_by(999, 999, 999, 999);
	assert_eq!(bbox, TileBBox::from_min_and_max(8, 0, 0, 255, 255).unwrap());

	// If bbox is empty, add_border should have no effect
	let mut empty_bbox = TileBBox::new_empty(8).unwrap();
	empty_bbox.expand_by(1, 2, 3, 4);
	assert_eq!(empty_bbox, TileBBox::new_empty(8).unwrap());
}

#[test]
fn test_shift_by() {
	let mut bbox = TileBBox::from_min_and_max(4, 1, 2, 3, 4).unwrap();
	bbox.shift_by(1, 1).unwrap();
	assert_eq!(bbox, TileBBox::from_min_and_max(4, 2, 3, 4, 5).unwrap());
}

#[test]
fn test_set_empty() {
	let mut bbox = TileBBox::from_min_and_max(4, 0, 0, 15, 15).unwrap();
	bbox.set_empty();
	assert!(bbox.is_empty());
}

#[test]
fn test_set_full() {
	let mut bbox = TileBBox::new_empty(4).unwrap();
	bbox.set_full();
	assert!(bbox.is_full());
}

#[test]
fn test_is_full() {
	let bbox = TileBBox::new_full(4).unwrap();
	assert!(bbox.is_full(), "Expected bbox ({bbox:?}) to be full");
}

#[test]
fn test_include_tile() {
	let mut bbox = TileBBox::from_min_and_max(6, 5, 10, 20, 30).unwrap();
	bbox.include(25, 35);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 25, 35).unwrap());
}

#[test]
fn test_include_bbox() {
	let mut bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13).unwrap();
	let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12).unwrap();
	bbox1.include_bbox(&bbox2).unwrap();
	assert_eq!(bbox1, TileBBox::from_min_and_max(4, 0, 10, 3, 13).unwrap());
}

#[test]
fn test_intersect_bbox() {
	let mut bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13).unwrap();
	let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12).unwrap();
	bbox1.intersect_with(&bbox2).unwrap();
	assert_eq!(bbox1, TileBBox::from_min_and_max(4, 1, 11, 2, 12).unwrap());
}

#[test]
fn test_overlaps_bbox() {
	let bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13).unwrap();
	let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12).unwrap();
	assert!(bbox1.overlaps_bbox(&bbox2).unwrap());

	let bbox3 = TileBBox::from_min_and_max(4, 8, 8, 9, 9).unwrap();
	assert!(!bbox1.overlaps_bbox(&bbox3).unwrap());
}

#[rstest]
#[case((8, 100, 100, 199, 199), (8, 100, 100), 0)]
#[case((8, 100, 100, 199, 199), (8, 101, 100), 1)]
#[case((8, 100, 100, 199, 199), (8, 199, 100), 99)]
#[case((8, 100, 100, 199, 199), (8, 100, 101), 100)]
#[case((8, 100, 100, 199, 199), (8, 100, 199), 9900)]
#[case((8, 100, 100, 199, 199), (8, 199, 199), 9999)]
fn get_tile_index_cases(#[case] bbox: (u8, u32, u32, u32, u32), #[case] coord: (u8, u32, u32), #[case] expected: u64) {
	let (l, x0, y0, x1, y1) = bbox;
	let bbox = TileBBox::from_min_and_max(l, x0, y0, x1, y1).unwrap();
	let (cl, cx, cy) = coord;
	let tc = TileCoord::new(cl, cx, cy).unwrap();
	assert_eq!(bbox.index_of(&tc).unwrap(), expected);
}

#[test]
fn test_as_geo_bbox() {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12).unwrap();
	let geo_bbox = bbox.to_geo_bbox().unwrap();
	assert_eq!(
		geo_bbox.as_string_list(),
		"-67.5,-74.01954331150228,0,-40.97989806962013"
	);
}

#[test]
fn test_contains() {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12).unwrap();
	assert!(bbox.contains(&TileCoord::new(4, 6, 11).unwrap()));
	assert!(!bbox.contains(&TileCoord::new(4, 4, 9).unwrap()));
	assert!(!bbox.contains(&TileCoord::new(5, 6, 11).unwrap()));
}

#[test]
fn test_new_valid_bbox() {
	let bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap();
	assert_eq!(bbox.level, 6);
	assert_eq!(bbox.x_min().unwrap(), 5);
	assert_eq!(bbox.y_min().unwrap(), 10);
	assert_eq!(bbox.x_max().unwrap(), 15);
	assert_eq!(bbox.y_max().unwrap(), 20);
}

#[test]
fn test_new_invalid_level() {
	let result = TileBBox::from_min_and_max(32, 0, 0, 1, 1);
	assert!(result.is_err());
}

#[test]
fn test_new_invalid_coordinates() {
	let result = TileBBox::from_min_and_max(4, 10, 10, 5, 15);
	assert!(result.is_err());

	let result = TileBBox::from_min_and_max(4, 5, 15, 7, 10);
	assert!(result.is_err());

	let result = TileBBox::from_min_and_max(4, 0, 0, 16, 15); // x_max exceeds max for level 4
	assert!(result.is_err());
}

#[test]
fn test_new_full() {
	let bbox = TileBBox::new_full(4).unwrap();
	assert_eq!(bbox, TileBBox::from_min_and_max(4, 0, 0, 15, 15).unwrap());
	assert!(bbox.is_full());
}

#[test]
fn test_from_geo_valid() {
	let geo_bbox = GeoBBox::new(-180.0, -85.05112878, 180.0, 85.05112878).unwrap();
	let bbox = TileBBox::from_geo(2, &geo_bbox).unwrap();
	assert_eq!(bbox, TileBBox::from_min_and_max(2, 0, 0, 3, 3).unwrap());
}

#[test]
fn test_is_empty() {
	let empty_bbox = TileBBox::new_empty(4).unwrap();
	assert!(empty_bbox.is_empty());

	let non_empty_bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap();
	assert!(!non_empty_bbox.is_empty());
}

#[test]
fn test_width_height() {
	let bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap();
	assert_eq!(bbox.width(), 11);
	assert_eq!(bbox.height(), 11);

	let empty_bbox = TileBBox::new_empty(4).unwrap();
	assert_eq!(empty_bbox.width(), 0);
	assert_eq!(empty_bbox.height(), 0);
}

#[test]
fn test_count_tiles() {
	let bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap();
	assert_eq!(bbox.count_tiles(), 121);

	let empty_bbox = TileBBox::new_empty(4).unwrap();
	assert_eq!(empty_bbox.count_tiles(), 0);
}

#[test]
fn test_include() -> Result<()> {
	let mut bbox = TileBBox::new_empty(6)?;
	bbox.include(5, 10);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 5, 10).unwrap());

	bbox.include(15, 20);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap());

	bbox.include(10, 15);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap());

	Ok(())
}

#[test]
fn test_include_coord() -> Result<()> {
	let mut bbox = TileBBox::new_empty(6)?;
	let coord = TileCoord::new(6, 5, 10).unwrap();
	bbox.include_coord(&coord)?;
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 5, 10).unwrap());

	let coord = TileCoord::new(6, 15, 20).unwrap();
	bbox.include_coord(&coord)?;
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20).unwrap());

	// Attempt to include a coordinate with a different zoom level
	let coord_invalid = TileCoord::new(5, 10, 15).unwrap();
	let result = bbox.include_coord(&coord_invalid);
	assert!(result.is_err());

	Ok(())
}

#[test]
fn test_add_border() -> Result<()> {
	let mut bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;

	// Add a border within bounds
	bbox.expand_by(2, 3, 2, 3);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 3, 7, 17, 23).unwrap());

	// Add a border that exceeds bounds, should clamp to max
	bbox.expand_by(10, 10, 10, 10);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 0, 0, 27, 33).unwrap());

	// Add border to an empty bounding box, should have no effect
	let mut empty_bbox = TileBBox::new_empty(6)?;
	empty_bbox.expand_by(1, 1, 1, 1);
	assert!(empty_bbox.is_empty());

	// Attempt to add a border with zero values
	bbox.expand_by(0, 0, 0, 0);
	assert_eq!(bbox, TileBBox::from_min_and_max(6, 0, 0, 27, 33).unwrap());

	Ok(())
}

#[test]
fn should_include_bbox_correctly_with_valid_and_empty_bboxes() -> Result<()> {
	let mut bbox1 = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
	let bbox2 = TileBBox::from_min_and_max(6, 10, 15, 20, 25)?;

	bbox1.include_bbox(&bbox2)?;
	assert_eq!(bbox1, TileBBox::from_min_and_max(6, 5, 10, 20, 25).unwrap());

	// Including an empty bounding box should have no effect
	let empty_bbox = TileBBox::new_empty(6)?;
	bbox1.include_bbox(&empty_bbox)?;
	assert_eq!(bbox1, TileBBox::from_min_and_max(6, 5, 10, 20, 25).unwrap());

	// Attempting to include a bounding box with different zoom level
	let bbox_diff_level = TileBBox::from_min_and_max(5, 5, 10, 20, 25)?;
	let result = bbox1.include_bbox(&bbox_diff_level);
	assert!(result.is_err());

	Ok(())
}

#[test]
fn should_intersect_bboxes_correctly_and_handle_empty_and_different_levels() -> Result<()> {
	let mut bbox1 = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
	let bbox2 = TileBBox::from_min_and_max(6, 10, 15, 20, 25)?;
	let bbox3 = TileBBox::from_min_and_max(6, 16, 21, 20, 25)?;

	bbox1.intersect_with(&bbox2)?;
	assert_eq!(bbox1, TileBBox::from_min_and_max(6, 10, 15, 15, 20)?);

	// Intersect with a non-overlapping bounding box
	bbox1.intersect_with(&bbox3)?;
	assert!(bbox1.is_empty());

	// Attempting to intersect with a bounding box of different zoom level
	let bbox_diff_level = TileBBox::from_min_and_max(5, 10, 15, 15, 20)?;
	let result = bbox1.intersect_with(&bbox_diff_level);
	assert!(result.is_err());

	Ok(())
}

#[test]
fn should_correctly_determine_bbox_overlap() -> Result<()> {
	let bbox1 = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
	let bbox2 = TileBBox::from_min_and_max(6, 10, 15, 20, 25)?;
	let bbox3 = TileBBox::from_min_and_max(6, 16, 21, 20, 25)?;
	let bbox4 = TileBBox::from_min_and_max(5, 10, 15, 15, 20)?;

	assert!(bbox1.overlaps_bbox(&bbox2)?);
	assert!(!bbox1.overlaps_bbox(&bbox3)?);
	assert!(bbox1.overlaps_bbox(&bbox1)?);
	assert!(bbox1.overlaps_bbox(&bbox1.clone())?);

	// Overlaps with a bounding box of different zoom level
	let result = bbox1.overlaps_bbox(&bbox4);
	assert!(result.is_err());

	Ok(())
}

#[test]
fn should_get_correct_tile_index() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;

	assert_eq!(bbox.index_of(&TileCoord::new(4, 5, 10).unwrap()).unwrap(), 0);
	assert_eq!(bbox.index_of(&TileCoord::new(4, 6, 10).unwrap()).unwrap(), 1);
	assert_eq!(bbox.index_of(&TileCoord::new(4, 7, 10).unwrap()).unwrap(), 2);
	assert_eq!(bbox.index_of(&TileCoord::new(4, 5, 11).unwrap()).unwrap(), 3);
	assert_eq!(bbox.index_of(&TileCoord::new(4, 7, 12).unwrap()).unwrap(), 8);

	// Attempt to get index of a coordinate outside the bounding box
	let coord_outside = TileCoord::new(4, 4, 9).unwrap();
	let result = bbox.index_of(&coord_outside);
	assert!(result.is_err());

	// Attempt to get index with mismatched zoom level
	let coord_diff_level = TileCoord::new(5, 5, 10).unwrap();
	let result = bbox.index_of(&coord_diff_level);
	assert!(result.is_err());

	Ok(())
}

#[rstest]
#[case(0, (4, 5, 10))]
#[case(1, (4, 6, 10))]
#[case(2, (4, 7, 10))]
#[case(3, (4, 5, 11))]
#[case(8, (4, 7, 12))]
fn get_coord_by_index_cases(#[case] index: u64, #[case] coord: (u8, u32, u32)) {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12).unwrap();
	let (l, x, y) = coord;
	assert_eq!(bbox.coord_at_index(index).unwrap(), TileCoord::new(l, x, y).unwrap());
}

#[test]
fn get_coord_by_index_out_of_bounds() {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12).unwrap();
	assert!(bbox.coord_at_index(9).is_err());
}

#[test]
fn should_convert_to_geo_bbox_correctly() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
	let geo_bbox = bbox.to_geo_bbox().unwrap();

	// Assuming TileCoord::as_geo() converts tile coordinates to geographical coordinates correctly,
	// the following is an example expected output. Adjust based on actual implementation.
	// For demonstration, let's assume:
	// - Tile (5, 10, 4) maps to longitude -67.5 and latitude 74.01954331
	// - Tile (7, 12, 4) maps to longitude 0.0 and latitude 40.97989807
	let expected_geo_bbox = GeoBBox::new(-67.5, -74.01954331150228, 0.0, -40.97989806962013)?;
	assert_eq!(geo_bbox, expected_geo_bbox);

	Ok(())
}

#[test]
fn should_determine_contains3_correctly() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
	let valid_coord = TileCoord::new(4, 6, 11).unwrap();
	let invalid_coord_zoom = TileCoord::new(5, 6, 11).unwrap();
	let invalid_coord_outside = TileCoord::new(4, 4, 9).unwrap();

	assert!(bbox.contains(&valid_coord));
	assert!(!bbox.contains(&invalid_coord_zoom));
	assert!(!bbox.contains(&invalid_coord_outside));

	Ok(())
}

#[test]
fn should_iterate_over_coords_correctly() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 6, 11)?;
	let coords: Vec<TileCoord> = bbox.iter_coords().collect();
	let expected_coords = vec![
		TileCoord::new(4, 5, 10).unwrap(),
		TileCoord::new(4, 6, 10).unwrap(),
		TileCoord::new(4, 5, 11).unwrap(),
		TileCoord::new(4, 6, 11).unwrap(),
	];
	assert_eq!(coords, expected_coords);

	Ok(())
}

#[test]
fn should_iterate_over_coords_correctly_when_consumed() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 6, 11)?;
	let coords: Vec<TileCoord> = bbox.into_iter_coords().collect();
	let expected_coords = vec![
		TileCoord::new(4, 5, 10).unwrap(),
		TileCoord::new(4, 6, 10).unwrap(),
		TileCoord::new(4, 5, 11).unwrap(),
		TileCoord::new(4, 6, 11).unwrap(),
	];
	assert_eq!(coords, expected_coords);

	Ok(())
}

#[test]
fn should_split_bbox_into_correct_grid() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 0, 0, 7, 7)?;

	let grid_size = 4;
	let grids: Vec<TileBBox> = bbox.iter_bbox_grid(grid_size).collect();

	let expected_grids = vec![
		TileBBox::from_min_and_max(4, 0, 0, 3, 3)?,
		TileBBox::from_min_and_max(4, 4, 0, 7, 3)?,
		TileBBox::from_min_and_max(4, 0, 4, 3, 7)?,
		TileBBox::from_min_and_max(4, 4, 4, 7, 7)?,
	];

	assert_eq!(grids, expected_grids);

	Ok(())
}

#[test]
fn should_scale_down_correctly() -> Result<()> {
	let mut bbox = TileBBox::from_min_and_max(4, 4, 4, 7, 7)?;
	bbox.scale_down(2);
	assert_eq!(bbox, TileBBox::from_min_and_max(4, 2, 2, 3, 3)?);

	// Scaling down by a factor larger than the coordinates
	bbox.scale_down(4);
	assert_eq!(bbox, TileBBox::from_min_and_max(4, 0, 0, 0, 0)?);

	Ok(())
}

#[test]
fn test_scaled_down_returns_new_bbox_and_preserves_original() -> Result<()> {
	// Original bbox
	let original = TileBBox::from_min_and_max(5, 10, 15, 20, 25)?;
	// scaled_down should return a new bbox without modifying the original
	let scaled = original.scaled_down(4);
	// Coordinates divided by 4: 10/4=2,15/4=3,20/4=5,25/4=6
	assert_eq!(scaled, TileBBox::from_min_and_max(5, 2, 3, 5, 6)?);
	// Original remains unchanged
	assert_eq!(original, TileBBox::from_min_and_max(5, 10, 15, 20, 25)?);
	// Scaling by 1 should produce identical bbox
	let same = original.scaled_down(1);
	assert_eq!(same, original);
	Ok(())
}

#[rstest]
#[case((0, 11, 0, 2))]
#[case((1, 12, 0, 3))]
#[case((2, 13, 0, 3))]
#[case((3, 14, 0, 3))]
#[case((4, 15, 1, 3))]
#[case((5, 16, 1, 4))]
#[case((6, 17, 1, 4))]
#[case((7, 18, 1, 4))]
#[case((8, 19, 2, 4))]
fn test_scale_down_cases(#[case] args: (u32, u32, u32, u32)) {
	let (min0, max0, min1, max1) = args;
	let mut bbox0 = TileBBox::from_min_and_max(8, min0, min0, max0, max0).unwrap();
	let bbox1 = TileBBox::from_min_and_max(8, min1, min1, max1, max1).unwrap();
	assert_eq!(
		bbox0.scaled_down(4),
		bbox1,
		"scaled_down(4) of {bbox0:?} should return {bbox1:?}"
	);
	bbox0.scale_down(4);
	assert_eq!(bbox0, bbox1, "scale_down(4) of {bbox0:?} should result in {bbox1:?}");
}

#[test]
fn should_shift_bbox_correctly() -> Result<()> {
	let mut bbox = TileBBox::from_min_and_size(6, 5, 10, 10, 10)?;
	bbox.shift_by(3, 4)?;
	assert_eq!(bbox, TileBBox::from_min_and_size(6, 8, 14, 10, 10)?);

	// Shifting beyond max should not cause overflow due to saturating_add
	let mut bbox = TileBBox::from_min_and_size(6, 14, 14, 10, 10)?;
	bbox.shift_by(2, 2)?;
	assert_eq!(bbox, TileBBox::from_min_and_size(6, 16, 16, 10, 10)?);

	let mut bbox = TileBBox::from_min_and_size(6, 5, 10, 10, 10)?;
	bbox.shift_by(-3, -5)?;
	assert_eq!(bbox, TileBBox::from_min_and_size(6, 2, 5, 10, 10)?);

	// Subtracting more than current coordinates should saturate at 0
	bbox.shift_by(-5, -10)?;
	assert_eq!(bbox, TileBBox::from_min_and_size(6, 0, 0, 10, 10)?);

	Ok(())
}

#[test]
fn should_handle_bbox_overlap_edge_cases() -> Result<()> {
	let bbox1 = TileBBox::from_min_and_max(4, 0, 0, 5, 5)?;
	let bbox2 = TileBBox::from_min_and_max(4, 5, 5, 10, 10)?;
	let bbox3 = TileBBox::from_min_and_max(4, 6, 6, 10, 10)?;
	let bbox4 = TileBBox::from_min_and_max(4, 0, 0, 5, 5)?;

	// Overlapping at the edge
	assert!(bbox1.overlaps_bbox(&bbox2)?);

	// No overlapping
	assert!(!bbox1.overlaps_bbox(&bbox3)?);

	// Completely overlapping
	assert!(bbox1.overlaps_bbox(&bbox4)?);

	// One empty bounding box
	let empty_bbox = TileBBox::new_empty(4)?;
	assert!(!bbox1.overlaps_bbox(&empty_bbox)?);

	Ok(())
}

#[test]
fn should_handle_empty_bbox_in_grid_iteration() -> Result<()> {
	let bbox = TileBBox::new_empty(4)?;
	let grids: Vec<TileBBox> = bbox.iter_bbox_grid(4).collect();
	assert!(grids.is_empty());
	Ok(())
}

#[test]
fn should_handle_single_tile_in_grid_iteration() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 5, 10, 5, 10)?;
	let grids: Vec<TileBBox> = bbox.iter_bbox_grid(4).collect();
	let expected_grids = vec![TileBBox::from_min_and_max(4, 5, 10, 5, 10).unwrap()];
	assert_eq!(grids, expected_grids);
	Ok(())
}

#[rstest]
#[case([1, 2, 16, 17], [0, 0, 19, 19])]
#[case([2, 3, 17, 18], [0, 0, 19, 19])]
#[case([3, 4, 18, 19], [0, 4, 19, 19])]
#[case([4, 5, 19, 20], [4, 4, 19, 23])]
#[case([5, 6, 20, 21], [4, 4, 23, 23])]
#[case([6, 7, 21, 22], [4, 4, 23, 23])]
#[case([7, 8, 22, 23], [4, 8, 23, 23])]
#[case([8, 9, 23, 24], [8, 8, 23, 27])]
fn test_round_shifting_cases(#[case] inp: [u32; 4], #[case] exp: [u32; 4]) {
	let bbox_exp = TileBBox::from_min_and_max(8, exp[0], exp[1], exp[2], exp[3]).unwrap();
	let mut bbox_inp = TileBBox::from_min_and_max(8, inp[0], inp[1], inp[2], inp[3]).unwrap();
	assert_eq!(bbox_inp.rounded(4), bbox_exp);
	bbox_inp.round(4);
	assert_eq!(bbox_inp, bbox_exp);
}

#[rstest]
#[case(1, [12, 34, 56, 78])]
#[case(2, [12, 34, 57, 79])]
#[case(3, [12, 33, 56, 80])]
#[case(4, [12, 32, 59, 79])]
#[case(5, [10, 30, 59, 79])]
#[case(6, [12, 30, 59, 83])]
#[case(7, [7, 28, 62, 83])]
#[case(10, [10, 30, 59, 79])]
#[case(100, [0, 0, 99, 99])]
#[case(1024, [0, 0, 1023, 1023])]
fn test_round_scaling_cases(#[case] scale: u32, #[case] exp: [u32; 4]) {
	let bbox_exp = TileBBox::from_min_and_max(12, exp[0], exp[1], exp[2], exp[3]).unwrap();
	let mut bbox_inp = TileBBox::from_min_and_max(12, 12, 34, 56, 78).unwrap();
	assert_eq!(bbox_inp.rounded(scale), bbox_exp);
	bbox_inp.round(scale);
	assert_eq!(bbox_inp, bbox_exp);
}

#[rstest]
#[case((1, 0, 0, 1, 1), (1, 0, 0, 1, 1))]
#[case((2, 0, 0, 1, 1), (2, 0, 2, 1, 3))]
#[case((3, 0, 0, 1, 1), (3, 0, 6, 1, 7))]
#[case((9, 10, 0, 10, 511), (9, 10, 0, 10, 511))]
#[case((9, 0, 10, 511, 10), (9, 0, 501, 511, 501))]
fn bbox_flip_y(#[case] a: (u8, u32, u32, u32, u32), #[case] b: (u8, u32, u32, u32, u32)) {
	let mut t = TileBBox::from_min_and_max(a.0, a.1, a.2, a.3, a.4).unwrap();
	t.flip_y();

	assert_eq!(t, TileBBox::from_min_and_max(b.0, b.1, b.2, b.3, b.4).unwrap());
}

#[test]
fn bbox_swap_xy_transform() {
	let mut bbox = TileBBox::from_min_and_max(4, 1, 2, 3, 4).unwrap();
	bbox.swap_xy();
	assert_eq!(bbox, TileBBox::from_min_and_max(4, 2, 1, 4, 3).unwrap());
}

#[test]
fn set_width_height_clamp_to_bounds() {
	// level 4 → max coordinate = 15
	let mut bbox = TileBBox::from_min_and_size(4, 10, 10, 3, 3).unwrap(); // covers x=10..12, y=10..12
	bbox.set_width(10).unwrap(); // would exceed max → clamp to 10..15 → width = 6
	assert_eq!(bbox.as_array().unwrap(), [10, 10, 15, 12]);
	bbox.set_height(10).unwrap();
	assert_eq!(bbox.as_array().unwrap(), [10, 10, 15, 15]);
}

#[test]
fn set_min_max_keep_consistency() -> Result<()> {
	let mut bbox = TileBBox::from_min_and_max(5, 8, 9, 12, 13).unwrap(); // width=5, height=5
	// Move min right/up; max should remain the same
	bbox.set_x_min(10)?;
	bbox.set_y_min(11)?;
	assert_eq!(bbox.as_array().unwrap(), [10, 11, 12, 13]);
	// Move max left/down; min should remain the same
	bbox.set_x_max(11)?;
	bbox.set_y_max(12)?;
	assert_eq!(bbox.as_array().unwrap(), [10, 11, 11, 12]);
	// Setting max less than min should empty the dimension
	bbox.set_y_max(10)?;
	assert!(bbox.is_empty());
	assert!(bbox.set_x_max(9).is_err());
	Ok(())
}

#[rstest]
#[case(4, 6, 2, 3)]
#[case(5, 6, 2, 3)]
#[case(4, 7, 2, 3)]
#[case(5, 7, 2, 3)]
fn level_decrease(#[case] min_in: u32, #[case] max_in: u32, #[case] min_out: u32, #[case] max_out: u32) {
	let mut bbox = TileBBox::from_min_and_max(10, min_in, min_in, max_in, max_in).unwrap();
	bbox.level_down();
	assert_eq!(bbox.level, 9);
	assert_eq!(bbox.as_array().unwrap(), [min_out, min_out, max_out, max_out]);
}

#[rstest]
#[case(4, 6, 8, 13)]
#[case(5, 6, 10, 13)]
#[case(4, 7, 8, 15)]
#[case(5, 7, 10, 15)]
fn level_increase(#[case] min_in: u32, #[case] max_in: u32, #[case] min_out: u32, #[case] max_out: u32) {
	let mut bbox = TileBBox::from_min_and_max(10, min_in, min_in, max_in, max_in).unwrap();
	bbox.level_up();
	assert_eq!(bbox.level, 11);
	assert_eq!(bbox.as_array().unwrap(), [min_out, min_out, max_out, max_out]);
}

#[test]
fn level_increase_decrease_roundtrip() {
	let original = TileBBox::from_min_and_max(4, 5, 6, 7, 8).unwrap();
	let inc = original.leveled_up();
	assert_eq!(inc.level, 5);
	assert_eq!(inc.as_array().unwrap(), [10, 12, 15, 17]);
	let dec = inc.leveled_down();
	assert_eq!(dec, original);
}

#[rstest]
#[case(4, 5, 6, 7, 8, 3, 3)]
#[case(8, 0, 0, 0, 0, 1, 1)]
fn corners_and_dimensions(
	#[case] level: u8,
	#[case] x0: u32,
	#[case] y0: u32,
	#[case] x1: u32,
	#[case] y1: u32,
	#[case] width: u32,
	#[case] height: u32,
) {
	let bbox = TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap();
	assert_eq!(bbox.min_corner().unwrap(), TileCoord::new(level, x0, y0).unwrap());
	assert_eq!(bbox.max_corner().unwrap(), TileCoord::new(level, x1, y1).unwrap());
	assert_eq!(bbox.dimensions(), (width, height));
}

#[rstest]
#[case(0, 0, 0, 0, 0)]
#[case(4, 0, 7, 8, 15)]
#[case(5, 0, 15, 16, 31)]
#[case(6, 0, 31, 32, 63)]
#[case(7, 0, 62, 65, 127)]
#[case(8, 0, 124, 131, 255)]
#[case(10, 0, 496, 527, 1023)]
#[case(20, 0, 507904, 540671, 1048575)]
#[case(30, 0, 520093696, 553648127, 1073741823)]
fn as_level_up_and_down(#[case] level: u32, #[case] x0: u32, #[case] y0: u32, #[case] x1: u32, #[case] y1: u32) {
	let bbox = TileBBox::from_min_and_max(6, 0, 31, 32, 63).unwrap();
	let up = bbox.at_level(level as u8);
	assert_eq!(
		[
			u32::from(up.level),
			up.x_min().unwrap(),
			up.y_min().unwrap(),
			up.x_max().unwrap(),
			up.y_max().unwrap()
		],
		[level, x0, y0, x1, y1]
	);
}

#[test]
fn get_quadrant_happy_path() -> Result<()> {
	let bbox = TileBBox::from_min_and_max(4, 8, 12, 11, 15)?; // 4x4 → even
	assert_eq!(bbox.get_quadrant(0)?, TileBBox::from_min_and_max(4, 8, 12, 9, 13)?);
	assert_eq!(bbox.get_quadrant(1)?, TileBBox::from_min_and_max(4, 10, 12, 11, 13)?);
	assert_eq!(bbox.get_quadrant(2)?, TileBBox::from_min_and_max(4, 8, 14, 9, 15)?);
	assert_eq!(bbox.get_quadrant(3)?, TileBBox::from_min_and_max(4, 10, 14, 11, 15)?);
	Ok(())
}

#[test]
fn get_quadrant_errors() {
	// Empty bbox → Ok(empty)
	let empty = TileBBox::new_empty(4).unwrap();
	assert!(empty.get_quadrant(0).unwrap().is_empty());
	// Odd width/height → error
	let odd_w = TileBBox::from_min_and_max(4, 0, 0, 2, 3).unwrap(); // width=3
	assert!(odd_w.get_quadrant(0).is_err());
	let odd_h = TileBBox::from_min_and_max(4, 0, 0, 3, 2).unwrap(); // height=3
	assert!(odd_h.get_quadrant(0).is_err());
	// Invalid quadrant index
	let even = TileBBox::from_min_and_max(4, 0, 0, 3, 3).unwrap();
	assert!(even.get_quadrant(4).is_err());
}

#[test]
fn max_value_and_string() {
	let bbox = TileBBox::from_min_and_max(5, 1, 2, 3, 4).unwrap();
	assert_eq!(bbox.max_coord(), (1u32 << 5) - 1);
	assert_eq!(bbox.to_string(), "5:[1,2,3,4]");
}

// --- test_scaled_up_cases ---
#[rstest]
#[case((5, 5, 10, 7, 12), 2, (5, 10, 20, 15, 25))]
#[case((4, 1, 1, 2, 2), 4, (4, 4, 4, 11, 11))]
#[case((8, 0, 0, 0, 0), 8, (8, 0, 0, 7, 7))]
#[case((6, 3, 5, 3, 5), 2, (6, 6, 10, 7, 11))]
fn test_scaled_up_cases(
	#[case] input: (u8, u32, u32, u32, u32),
	#[case] scale: u32,
	#[case] expected: (u8, u32, u32, u32, u32),
) -> Result<()> {
	let (level, x0, y0, x1, y1) = input;
	let bbox = TileBBox::from_min_and_max(level, x0, y0, x1, y1)?;
	let scaled = bbox.scaled_up(scale)?;
	let (exp_level, exp_x0, exp_y0, exp_x1, exp_y1) = expected;
	assert_eq!(scaled.level, exp_level);
	assert_eq!(scaled.as_array()?, [exp_x0, exp_y0, exp_x1, exp_y1]);
	// Ensure original bbox remains unchanged
	assert_eq!(bbox, TileBBox::from_min_and_max(level, x0, y0, x1, y1)?);
	Ok(())
}

#[test]
fn test_intersect_with_pyramid() -> Result<()> {
	// Create a pyramid with a known full bbox at level 5
	let pyramid = TileBBoxPyramid::from(&[TileBBox::new_full(5)?]);

	// Create a bbox partially overlapping the full bbox
	let mut bbox = TileBBox::from_min_and_max(5, 10, 10, 20, 20)?;
	bbox.intersect_with_pyramid(&pyramid);

	// Since the pyramid covers the full range, intersection should not modify bbox
	assert_eq!(bbox, TileBBox::from_min_and_max(5, 10, 10, 20, 20)?);

	// Now create a pyramid with a smaller bbox (subset)
	let smaller_bbox = TileBBox::from_min_and_max(5, 12, 12, 18, 18)?;
	let pyramid_small = TileBBoxPyramid::from(&[smaller_bbox]);
	let mut bbox = TileBBox::from_min_and_max(5, 10, 10, 20, 20)?;
	bbox.intersect_with_pyramid(&pyramid_small);

	// Intersection should shrink to overlap region
	assert_eq!(bbox, TileBBox::from_min_and_max(5, 12, 12, 18, 18)?);

	Ok(())
}

#[test]
fn test_try_contains_bbox() -> Result<()> {
	let bbox_outer = TileBBox::from_min_and_max(5, 10, 10, 20, 20)?;
	let bbox_inner = TileBBox::from_min_and_max(5, 12, 12, 18, 18)?;
	let bbox_partial = TileBBox::from_min_and_max(5, 15, 15, 25, 25)?;
	let bbox_non_overlap = TileBBox::from_min_and_max(5, 21, 21, 22, 22)?;
	let bbox_diff_level = TileBBox::from_min_and_max(6, 12, 12, 18, 18)?;

	// Fully contained
	assert!(bbox_outer.try_contains_bbox(&bbox_inner)?);
	// Not fully contained (partial overlap)
	assert!(!bbox_outer.try_contains_bbox(&bbox_partial)?);
	// Not contained (no overlap)
	assert!(!bbox_outer.try_contains_bbox(&bbox_non_overlap)?);

	// Empty bboxes always false
	let empty_outer = TileBBox::new_empty(5)?;
	let empty_inner = TileBBox::new_empty(5)?;
	assert!(!empty_outer.try_contains_bbox(&bbox_inner)?);
	assert!(!bbox_outer.try_contains_bbox(&empty_inner)?);

	// Different zoom levels → error
	assert!(bbox_outer.try_contains_bbox(&bbox_diff_level).is_err());

	Ok(())
}
