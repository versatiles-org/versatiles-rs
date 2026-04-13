//! Constructors for [`TileQuadtree`].

use super::{BBox, Node, TileQuadtree};
use crate::{GeoBBox, TileBBox, TileCoord, validate_zoom_level};
use anyhow::{Result, ensure};

impl TileQuadtree {
	/// Create an empty quadtree at the given zoom level.
	pub fn new_empty(zoom: u8) -> Result<Self> {
		validate_zoom_level(zoom)?;
		Ok(TileQuadtree {
			zoom,
			root: Node::Empty,
		})
	}

	/// Create a full quadtree (all tiles covered) at the given zoom level.
	pub fn new_full(zoom: u8) -> Result<Self> {
		validate_zoom_level(zoom)?;
		Ok(TileQuadtree { zoom, root: Node::Full })
	}

	/// Build a quadtree from a [`TileBBox`], covering exactly those tiles.
	///
	/// # Errors
	/// Returns an error if the bbox zoom level exceeds `MAX_ZOOM_LEVEL`.
	#[must_use]
	pub fn from_bbox(bbox: &TileBBox) -> Self {
		let zoom = bbox.level;
		validate_zoom_level(zoom).expect("TileBBox level should have been validated on construction");

		if bbox.is_empty() {
			return TileQuadtree::new_empty(zoom).unwrap();
		}

		let size = 1u64 << zoom;
		let x_min = u64::from(bbox.x_min().unwrap());
		let y_min = u64::from(bbox.y_min().unwrap());
		let x_max = u64::from(bbox.x_max().unwrap()) + 1; // exclusive
		let y_max = u64::from(bbox.y_max().unwrap()) + 1; // exclusive

		let root = build_node(
			zoom,
			0,
			0,
			size,
			BBox {
				x_min,
				y_min,
				x_max,
				y_max,
			},
		);
		TileQuadtree { zoom, root }
	}

	/// Build a quadtree from a geographic bounding box at the given zoom level.
	///
	/// # Errors
	/// Returns an error if the zoom level or geographic coordinates are invalid.
	pub fn from_geo(zoom: u8, bbox: &GeoBBox) -> Result<Self> {
		validate_zoom_level(zoom)?;
		let tile_bbox = TileBBox::from_geo(zoom, bbox)?;
		Ok(Self::from_bbox(&tile_bbox))
	}
}

/// Recursively build a quadtree node for the cell covering
/// `[x_off, x_off+size) × [y_off, y_off+size)` against the bbox
/// `[bbox.x_min, bbox.x_max) × [bbox.y_min, bbox.y_max)` (all exclusive on max side).
fn build_node(depth: u8, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> Node {
	// Intersection of bbox with this cell
	let ix_min = bbox.x_min.max(x_off);
	let iy_min = bbox.y_min.max(y_off);
	let ix_max = bbox.x_max.min(x_off + size);
	let iy_max = bbox.y_max.min(y_off + size);

	if ix_min >= ix_max || iy_min >= iy_max {
		return Node::Empty;
	}

	if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
		return Node::Full;
	}

	if depth == 0 {
		// We've reached leaf level — any intersection means Full
		return Node::Full;
	}

	let half = size / 2;
	let mid_x = x_off + half;
	let mid_y = y_off + half;
	Node::new_partial([
		build_node(depth - 1, x_off, y_off, half, bbox),
		build_node(depth - 1, mid_x, y_off, half, bbox),
		build_node(depth - 1, x_off, mid_y, half, bbox),
		build_node(depth - 1, mid_x, mid_y, half, bbox),
	])
}

/// Validate that a TileCoord belongs to the given zoom level.
pub(crate) fn check_coord_zoom(coord: &TileCoord, zoom: u8) -> Result<()> {
	ensure!(
		coord.level == zoom,
		"TileCoord level {} does not match quadtree zoom {}",
		coord.level,
		zoom
	);
	Ok(())
}

/// Validate that a TileBBox belongs to the given zoom level.
pub(crate) fn check_bbox_zoom(bbox: &TileBBox, zoom: u8) -> Result<()> {
	ensure!(
		bbox.level == zoom,
		"TileBBox level {} does not match quadtree zoom {}",
		bbox.level,
		zoom
	);
	Ok(())
}
