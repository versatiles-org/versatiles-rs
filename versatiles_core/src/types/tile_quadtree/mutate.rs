//! Mutation methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, Node, TileQuadtree};
use crate::{TileBBox, TileCoord};
use anyhow::Result;

impl TileQuadtree {
	/// Insert a single tile into the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	pub fn include_coord(&mut self, coord: &TileCoord) -> Result<()> {
		check_coord_zoom(coord, self.level)?;
		let size = 1u64 << self.level;
		self
			.root
			.insert_coord((0, 0), size, (u64::from(coord.x), u64::from(coord.y)));
		Ok(())
	}

	/// Insert all tiles within a [`TileBBox`] into the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		let size = 1u64 << self.level;
		let Some(bbox) = BBox::new(bbox) else {
			return Ok(());
		};
		self.root.include_bbox((0, 0), size, &bbox);
		Ok(())
	}

	/// Expands coverage outward by `size` tiles in all directions.
	///
	/// Uses Full-node decomposition: dilation distributes over union, so each
	/// `Full` subtree (an exact rectangle) can be expanded independently and
	/// re-inserted. Complexity is O(N · zoom) where N is the number of tree nodes,
	/// far better than the O(T · zoom) per-tile alternative.
	pub fn buffer(&mut self, size: u32) {
		if size == 0 || self.is_empty() {
			return;
		}
		let tree_size = 1u64 << self.level;
		let n = u64::from(size);

		let mut rects: Vec<BBox> = Vec::new();
		self.root.collect_full_rects((0, 0), tree_size, &mut rects);

		let mut new_root = Node::Empty;
		for rect in rects {
			new_root.include_bbox(
				(0, 0),
				tree_size,
				&BBox {
					x_min: rect.x_min.saturating_sub(n),
					y_min: rect.y_min.saturating_sub(n),
					x_max: (rect.x_max + n).min(tree_size),
					y_max: (rect.y_max + n).min(tree_size),
				},
			);
		}
		self.root = new_root;
	}

	/// Remove a single tile from the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	pub fn remove_coord(&mut self, coord: &TileCoord) -> Result<()> {
		check_coord_zoom(coord, self.level)?;
		let size = 1u64 << self.level;
		self
			.root
			.remove_coord((0, 0), size, (u64::from(coord.x), u64::from(coord.y)));
		Ok(())
	}

	/// Remove all tiles within a [`TileBBox`] from the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		let Some(bbox) = BBox::new(bbox) else {
			return Ok(());
		};
		self.root.remove_bbox((0, 0), 1u64 << self.level, &bbox);
		Ok(())
	}

	/// Flips all tile coordinates vertically: `y → (2^level − 1 − y)`.
	///
	/// Recurses through the tree, swapping top and bottom quadrant pairs at
	/// each `Partial` node. `Full` and `Empty` nodes are unaffected.
	pub fn flip_y(&mut self) {
		self.root.flip_y();
	}

	/// Swaps x and y coordinates for all tiles: `(x, y) → (y, x)`.
	///
	/// Recurses through the tree, exchanging the NE and SW quadrants at each
	/// `Partial` node. `Full` and `Empty` nodes are unaffected.
	pub fn swap_xy(&mut self) {
		self.root.swap_xy();
	}

	/// Intersects the quadtree with a [`TileBBox`], removing any tiles outside it.
	///
	/// If `bbox` is empty, the entire tree is cleared. Otherwise, each branch of
	/// the tree is recursively clipped to the intersection region.
	///
	/// # Errors
	/// Returns an error if `bbox`'s zoom level doesn't match.
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		let Some(bbox) = BBox::new(bbox) else {
			self.root = Node::Empty;
			return Ok(());
		};
		self.root.intersect_bbox((0, 0), 1u64 << self.level, &bbox);
		Ok(())
	}
}
