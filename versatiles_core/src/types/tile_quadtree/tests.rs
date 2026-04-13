//! Tests for [`TileQuadtree`].

use super::*;
use crate::{GeoBBox, TileBBox, TileCoord};
use anyhow::Result;

fn coord(level: u8, x: u32, y: u32) -> TileCoord {
	TileCoord::new(level, x, y).unwrap()
}

fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
	TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
}

// -------------------------------------------------------------------------
// Constructors
// -------------------------------------------------------------------------

#[test]
fn new_empty_and_full() {
	let e = TileQuadtree::new_empty(4);
	assert!(e.is_empty());
	assert!(!e.is_full());
	assert_eq!(e.zoom(), 4);
	assert_eq!(e.count_tiles(), 0);

	let f = TileQuadtree::new_full(3);
	assert!(!f.is_empty());
	assert!(f.is_full());
	assert_eq!(f.zoom(), 3);
	assert_eq!(f.count_tiles(), 64); // 8×8
}

#[test]
fn from_bbox_empty() -> Result<()> {
	let b = TileBBox::new_empty(5)?;
	let t = TileQuadtree::from_bbox(&b)?;
	assert!(t.is_empty());
	Ok(())
}

#[test]
fn from_bbox_full() -> Result<()> {
	let b = TileBBox::new_full(3)?;
	let t = TileQuadtree::from_bbox(&b)?;
	assert!(t.is_full());
	assert_eq!(t.count_tiles(), 64);
	Ok(())
}

#[test]
fn from_bbox_partial() -> Result<()> {
	// z=2: 4×4 grid, bbox covers x=0..1, y=0..1 (2×2 = 4 tiles)
	let b = bbox(2, 0, 0, 1, 1);
	let t = TileQuadtree::from_bbox(&b)?;
	assert!(!t.is_empty());
	assert!(!t.is_full());
	assert_eq!(t.count_tiles(), 4);
	Ok(())
}

#[test]
fn from_geo() -> Result<()> {
	let geo = GeoBBox::new(8.0, 51.0, 8.5, 51.5).unwrap();
	let t = TileQuadtree::from_geo(9, &geo)?;
	assert!(!t.is_empty());
	Ok(())
}

// -------------------------------------------------------------------------
// Queries
// -------------------------------------------------------------------------

#[test]
fn tile_count_full() {
	for z in 0u8..=5 {
		let expected = 1u64 << (2 * u32::from(z));
		assert_eq!(TileQuadtree::new_full(z).count_tiles(), expected);
	}
}

#[test]
fn bounds_empty_and_full() -> Result<()> {
	assert!(TileQuadtree::new_empty(3).bounds().is_none());
	let full_bounds = TileQuadtree::new_full(2).bounds().unwrap();
	assert_eq!(full_bounds, TileBBox::new_full(2)?);
	Ok(())
}

#[test]
fn bounds_partial() -> Result<()> {
	let b = bbox(4, 3, 5, 7, 9);
	let t = TileQuadtree::from_bbox(&b)?;
	let bounds = t.bounds().unwrap();
	assert_eq!(bounds, b);
	Ok(())
}

#[test]
fn contains_tile() -> Result<()> {
	let t = TileQuadtree::from_bbox(&bbox(3, 2, 2, 4, 4))?;
	assert!(t.includes_coord(&coord(3, 2, 2))?);
	assert!(t.includes_coord(&coord(3, 4, 4))?);
	assert!(!t.includes_coord(&coord(3, 0, 0))?);
	assert!(!t.includes_coord(&coord(3, 5, 5))?);
	// Wrong zoom
	assert!(t.includes_coord(&coord(4, 2, 2)).is_err());
	Ok(())
}

#[test]
fn contains_bbox() -> Result<()> {
	let full = TileQuadtree::new_full(3);
	assert!(full.includes_bbox(&TileBBox::new_full(3)?)?);
	assert!(full.includes_bbox(&bbox(3, 0, 0, 3, 3))?);

	let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3))?;
	assert!(t.includes_bbox(&bbox(3, 0, 0, 2, 2))?);
	assert!(!t.includes_bbox(&TileBBox::new_full(3)?)?);
	Ok(())
}

#[test]
fn intersects() -> Result<()> {
	let a = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3))?;
	let b = TileQuadtree::from_bbox(&bbox(3, 3, 3, 7, 7))?;
	let c = TileQuadtree::from_bbox(&bbox(3, 4, 4, 7, 7))?;
	assert!(a.intersects_tree(&b)?);
	assert!(!a.intersects_tree(&c)?);
	assert!(a.intersects_tree(&TileQuadtree::new_full(3))?);
	assert!(!a.intersects_tree(&TileQuadtree::new_empty(3))?);
	Ok(())
}

// -------------------------------------------------------------------------
// Mutation
// -------------------------------------------------------------------------

#[test]
fn insert_tile() -> Result<()> {
	let mut t = TileQuadtree::new_empty(3);
	t.include_coord(&coord(3, 0, 0))?;
	assert_eq!(t.count_tiles(), 1);
	assert!(t.includes_coord(&coord(3, 0, 0))?);
	assert!(!t.includes_coord(&coord(3, 1, 0))?);
	Ok(())
}

#[test]
fn insert_tile_collapses_to_full() -> Result<()> {
	// At zoom 1, there are only 4 tiles. Insert all 4.
	let mut t = TileQuadtree::new_empty(1);
	t.include_coord(&coord(1, 0, 0))?;
	t.include_coord(&coord(1, 1, 0))?;
	t.include_coord(&coord(1, 0, 1))?;
	t.include_coord(&coord(1, 1, 1))?;
	assert!(t.is_full());
	Ok(())
}

#[test]
fn insert_bbox() -> Result<()> {
	let mut t = TileQuadtree::new_empty(4);
	t.include_bbox(&bbox(4, 0, 0, 7, 7))?;
	assert_eq!(t.count_tiles(), 64);
	Ok(())
}

#[test]
fn remove_tile() -> Result<()> {
	let mut t = TileQuadtree::new_full(2);
	t.remove_coord(&coord(2, 0, 0))?;
	assert!(!t.is_full());
	assert_eq!(t.count_tiles(), 15);
	assert!(!t.includes_coord(&coord(2, 0, 0))?);
	assert!(t.includes_coord(&coord(2, 1, 0))?);
	Ok(())
}

#[test]
fn remove_bbox() -> Result<()> {
	let mut t = TileQuadtree::new_full(3);
	t.remove_bbox(&bbox(3, 0, 0, 3, 7))?;
	assert!(!t.is_full());
	assert_eq!(t.count_tiles(), 32); // half the tiles removed
	Ok(())
}

// -------------------------------------------------------------------------
// Set operations
// -------------------------------------------------------------------------

#[test]
fn union() -> Result<()> {
	let a = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 7))?;
	let b = TileQuadtree::from_bbox(&bbox(3, 4, 0, 7, 7))?;
	let u = a.union(&b)?;
	assert!(u.is_full());
	Ok(())
}

#[test]
fn intersection() -> Result<()> {
	let a = TileQuadtree::from_bbox(&bbox(3, 0, 0, 5, 5))?;
	let b = TileQuadtree::from_bbox(&bbox(3, 3, 3, 7, 7))?;
	let i = a.intersection(&b)?;
	// Overlap is [3,3] to [5,5] = 3x3 = 9 tiles
	assert_eq!(i.count_tiles(), 9);
	Ok(())
}

#[test]
fn difference() -> Result<()> {
	let a = TileQuadtree::new_full(2);
	let b = TileQuadtree::from_bbox(&bbox(2, 0, 0, 1, 1))?; // 4 tiles
	let d = a.difference(&b)?;
	assert_eq!(d.count_tiles(), 12);
	Ok(())
}

#[test]
fn set_ops_zoom_mismatch() {
	let a = TileQuadtree::new_full(3);
	let b = TileQuadtree::new_full(4);
	assert!(a.union(&b).is_err());
	assert!(a.intersection(&b).is_err());
	assert!(a.difference(&b).is_err());
}

// -------------------------------------------------------------------------
// Zoom
// -------------------------------------------------------------------------

#[test]
fn level_up_full_empty() {
	assert!(TileQuadtree::new_full(5).level_up().is_full());
	assert!(TileQuadtree::new_empty(5).level_up().is_empty());
}

#[test]
fn level_down_full_empty() {
	let f = TileQuadtree::new_full(3).level_down();
	assert_eq!(f.zoom(), 4);
	assert!(f.is_full());
}

#[test]
fn level_up_zoom0_unchanged() {
	let t = TileQuadtree::new_full(0);
	let up = t.level_up();
	assert_eq!(up.zoom(), 0);
}

#[test]
fn at_level_roundtrip() -> Result<()> {
	let t = TileQuadtree::from_bbox(&bbox(4, 4, 4, 11, 11))?;
	let up = t.at_level(3);
	assert_eq!(up.zoom(), 3);
	let down = t.at_level(5);
	assert_eq!(down.zoom(), 5);
	// Going up should have fewer or equal tiles
	assert!(up.count_tiles() <= t.count_tiles());
	Ok(())
}

// -------------------------------------------------------------------------
// Iteration
// -------------------------------------------------------------------------

#[test]
fn iter_tiles_count() -> Result<()> {
	let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3))?;
	let tiles: Vec<_> = t.iter_coords().collect();
	assert_eq!(tiles.len() as u64, t.count_tiles());
	assert_eq!(tiles.len(), 16);
	Ok(())
}

#[test]
fn iter_tiles_full() {
	let t = TileQuadtree::new_full(2);
	let mut tiles: Vec<_> = t.iter_coords().collect();
	tiles.sort_by_key(|c| (c.y, c.x));
	let mut expected: Vec<_> = (0..4)
		.flat_map(|y| (0..4u32).map(move |x| TileCoord::new(2, x, y).unwrap()))
		.collect();
	expected.sort_by_key(|c| (c.y, c.x));
	assert_eq!(tiles, expected);
}

#[test]
fn iter_tiles_empty() {
	let t = TileQuadtree::new_empty(3);
	assert_eq!(t.iter_coords().count(), 0);
}

#[test]
fn iter_bbox_grid_covers_all() -> Result<()> {
	let t = TileQuadtree::from_bbox(&bbox(4, 0, 0, 15, 15))?;
	let mut total = 0u64;
	for cell in t.iter_bbox_grid(4) {
		total += cell.count_tiles();
	}
	assert_eq!(total, t.count_tiles());
	Ok(())
}

// -------------------------------------------------------------------------
// Serialization
// -------------------------------------------------------------------------

#[test]
fn serialize_roundtrip_empty() -> Result<()> {
	let t = TileQuadtree::new_empty(5);
	let bytes = t.serialize();
	let t2 = TileQuadtree::deserialize(5, &bytes)?;
	assert_eq!(t, t2);
	Ok(())
}

#[test]
fn serialize_roundtrip_full() -> Result<()> {
	let t = TileQuadtree::new_full(4);
	let bytes = t.serialize();
	let t2 = TileQuadtree::deserialize(4, &bytes)?;
	assert_eq!(t, t2);
	Ok(())
}

#[test]
fn serialize_roundtrip_partial() -> Result<()> {
	let t = TileQuadtree::from_bbox(&bbox(4, 3, 5, 11, 12))?;
	let bytes = t.serialize();
	let t2 = TileQuadtree::deserialize(4, &bytes)?;
	assert_eq!(t, t2);
	assert_eq!(t.count_tiles(), t2.count_tiles());
	Ok(())
}

#[test]
fn deserialize_zoom_mismatch() {
	let t = TileQuadtree::new_full(3);
	let bytes = t.serialize();
	assert!(TileQuadtree::deserialize(4, &bytes).is_err());
}

// -------------------------------------------------------------------------
// count_nodes
// -------------------------------------------------------------------------

#[test]
fn count_nodes_empty_and_full() {
	// An empty tree has 1 node (the root Empty node).
	assert_eq!(TileQuadtree::new_empty(4).count_nodes(), 1);
	// A full tree has 1 node (the root Full node).
	assert_eq!(TileQuadtree::new_full(5).count_nodes(), 1);
}

#[test]
fn count_nodes_partial_tree() -> Result<()> {
	// A tree with a partial subtree has more than 1 node.
	let mut t = TileQuadtree::new_empty(3);
	t.include_coord(&coord(3, 0, 0))?;
	assert!(t.count_nodes() > 1, "partial tree should have more than one node");
	Ok(())
}

// -------------------------------------------------------------------------
// to_geo_bbox
// -------------------------------------------------------------------------

#[test]
fn to_geo_bbox_empty_is_none() {
	assert!(TileQuadtree::new_empty(4).to_geo_bbox().is_none());
}

#[test]
fn to_geo_bbox_full_covers_world() {
	let geo = TileQuadtree::new_full(0).to_geo_bbox().unwrap();
	assert!(geo.x_min <= -179.0);
	assert!(geo.x_max >= 179.0);
}

// -------------------------------------------------------------------------
// Zoom-mismatch errors
// -------------------------------------------------------------------------

#[test]
fn zoom_mismatch_includes_bbox() {
	let t = TileQuadtree::new_full(3);
	assert!(t.includes_bbox(&bbox(4, 0, 0, 1, 1)).is_err());
}

#[test]
fn zoom_mismatch_include_coord() {
	let mut t = TileQuadtree::new_empty(3);
	assert!(t.include_coord(&coord(4, 0, 0)).is_err());
}

#[test]
fn zoom_mismatch_include_bbox() {
	let mut t = TileQuadtree::new_empty(3);
	assert!(t.include_bbox(&bbox(4, 0, 0, 1, 1)).is_err());
}

#[test]
fn zoom_mismatch_remove_coord() {
	let mut t = TileQuadtree::new_full(3);
	assert!(t.remove_coord(&coord(4, 0, 0)).is_err());
}

#[test]
fn zoom_mismatch_remove_bbox() {
	let mut t = TileQuadtree::new_full(3);
	assert!(t.remove_bbox(&bbox(4, 0, 0, 1, 1)).is_err());
}

// -------------------------------------------------------------------------
// Empty-bbox no-ops
// -------------------------------------------------------------------------

#[test]
fn include_empty_bbox_is_noop() -> Result<()> {
	let mut t = TileQuadtree::new_empty(4);
	t.include_bbox(&TileBBox::new_empty(4)?)?;
	assert!(t.is_empty());
	Ok(())
}

#[test]
fn remove_empty_bbox_is_noop() -> Result<()> {
	let mut t = TileQuadtree::new_full(3);
	let count_before = t.count_tiles();
	t.remove_bbox(&TileBBox::new_empty(3)?)?;
	assert_eq!(t.count_tiles(), count_before);
	Ok(())
}

// -------------------------------------------------------------------------
// includes_bbox with empty input
// -------------------------------------------------------------------------

#[test]
fn includes_empty_bbox_returns_true() -> Result<()> {
	// An empty bbox is trivially contained in any tree.
	let t = TileQuadtree::new_empty(4);
	assert!(t.includes_bbox(&TileBBox::new_empty(4)?)?);
	Ok(())
}

// -------------------------------------------------------------------------
// Display
// -------------------------------------------------------------------------

#[test]
fn display() {
	let t = TileQuadtree::new_full(3);
	let s = format!("{t}");
	assert!(s.contains("zoom=3"));
	assert!(s.contains("tiles=64"));
}
