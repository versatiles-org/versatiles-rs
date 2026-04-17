//! Mutation methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, Node, TileQuadtree};
use crate::{TileBBox, TileCoord};
use anyhow::Result;
use versatiles_derive::context;

impl TileQuadtree {
	/// Insert a single tile into the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	#[context("Failed to include TileCoord {coord:?} into TileQuadtree at level {}", self.level)]
	pub fn insert_coord(&mut self, coord: &TileCoord) -> Result<()> {
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
	#[context("Failed to include TileBBox {bbox:?} into TileQuadtree at level {}", self.level)]
	pub fn insert_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		let size = 1u64 << self.level;
		let Some(bbox) = BBox::new(bbox) else {
			return Ok(());
		};
		self.root.insert_bbox((0, 0), size, &bbox);
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
			new_root.insert_bbox(
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
	#[context("Failed to remove TileCoord {coord:?} from TileQuadtree at level {}", self.level)]
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
	#[context("Failed to remove TileBBox {bbox:?} from TileQuadtree at level {}", self.level)]
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
	#[context("Failed to intersect TileQuadtree at level {} with TileBBox {bbox:?}", self.level)]
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

impl Node {
	pub fn intersect_bbox(&mut self, (x_off, y_off): (u64, u64), size: u64, bbox: &BBox) {
		if self == &Node::Empty {
			return;
		}
		// Clip bbox to this cell's region.
		let ix_min = bbox.x_min.max(x_off);
		let iy_min = bbox.y_min.max(y_off);
		let ix_max = bbox.x_max.min(x_off + size);
		let iy_max = bbox.y_max.min(y_off + size);

		// No overlap → clear this subtree entirely.
		if ix_min >= ix_max || iy_min >= iy_max {
			*self = Node::Empty;
			return;
		}

		if self == &Node::Full {
			if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
				// bbox covers the entire cell: stays Full.
				return;
			}
			// Materialise four Full children, then intersect each with bbox.
			*self = Node::new_partial_full();
		}

		if let Node::Partial(children) = self {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			children[0].intersect_bbox((x_off, y_off), half, bbox);
			children[1].intersect_bbox((mid_x, y_off), half, bbox);
			children[2].intersect_bbox((x_off, mid_y), half, bbox);
			children[3].intersect_bbox((mid_x, mid_y), half, bbox);
			self.normalize();
		}
	}

	pub fn insert_coord(&mut self, (x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) {
		match self {
			Node::Full => (),
			Node::Empty => {
				if size == 1 {
					*self = Node::Full;
				} else {
					*self = Node::new_partial_empty();
					self.insert_coord((x_off, y_off), size, (tx, ty));
				}
			}
			Node::Partial(children) => {
				let (idx, cx, cy, half) = Node::child_quadrant((x_off, y_off), size, (tx, ty));
				children[idx].insert_coord((cx, cy), half, (tx, ty));
				self.normalize();
			}
		}
	}

	pub fn insert_bbox(&mut self, (x_off, y_off): (u64, u64), size: u64, bbox: &BBox) {
		// Intersection of bbox with this cell
		let ix_min = bbox.x_min.max(x_off);
		let iy_min = bbox.y_min.max(y_off);
		let ix_max = bbox.x_max.min(x_off + size);
		let iy_max = bbox.y_max.min(y_off + size);

		if ix_min >= ix_max || iy_min >= iy_max {
			return; // bbox doesn't touch this cell
		}

		// If bbox covers the full cell, mark Full
		if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
			*self = Node::Full;
			return;
		}

		if self == &Node::Full {
			return;
		}
		if self == &Node::Empty {
			if size == 1 {
				*self = Node::Full;
				return;
			}
			*self = Node::new_partial_empty();
		}
		if let Node::Partial(children) = self {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			children[0].insert_bbox((x_off, y_off), half, bbox);
			children[1].insert_bbox((mid_x, y_off), half, bbox);
			children[2].insert_bbox((x_off, mid_y), half, bbox);
			children[3].insert_bbox((mid_x, mid_y), half, bbox);
			self.normalize();
		}
	}

	/// Collect the bounding rectangle of every `Full` subtree into `out`.
	///
	/// Each entry uses exclusive upper bounds, matching the internal [`BBox`]
	/// convention used throughout this module.
	pub(crate) fn collect_full_rects(&self, (x_off, y_off): (u64, u64), size: u64, out: &mut Vec<super::BBox>) {
		match self {
			Node::Empty => {}
			Node::Full => out.push(super::BBox {
				x_min: x_off,
				y_min: y_off,
				x_max: x_off + size,
				y_max: y_off + size,
			}),
			Node::Partial(children) => {
				let half = size / 2;
				let (mid_x, mid_y) = (x_off + half, y_off + half);
				children[0].collect_full_rects((x_off, y_off), half, out); // NW
				children[1].collect_full_rects((mid_x, y_off), half, out); // NE
				children[2].collect_full_rects((x_off, mid_y), half, out); // SW
				children[3].collect_full_rects((mid_x, mid_y), half, out); // SE
			}
		}
	}

	/// Flip all tile coordinates vertically: `y → (level_size − 1 − y)`.
	///
	/// At each `Partial` node the two top quadrants are swapped with the two
	/// bottom quadrants (`NW↔SW`, `NE↔SE`), then every child is recursed.
	/// `Full` and `Empty` nodes are symmetric — no-op.
	pub(crate) fn flip_y(&mut self) {
		if let Node::Partial(children) = self {
			{
				let s: &mut [Node] = &mut **children;
				s.swap(0, 2); // NW ↔ SW
				s.swap(1, 3); // NE ↔ SE
			}
			for child in children.iter_mut() {
				child.flip_y();
			}
		}
	}

	/// Swap x and y coordinates for all tiles: `(x, y) → (y, x)`.
	///
	/// At each `Partial` node the NE and SW quadrants are exchanged
	/// (`[1]↔[2]`), then every child is recursed.
	/// `Full` and `Empty` nodes are symmetric — no-op.
	pub(crate) fn swap_xy(&mut self) {
		if let Node::Partial(children) = self {
			{
				let s: &mut [Node] = &mut **children;
				s.swap(1, 2); // NE ↔ SW
			}
			for child in children.iter_mut() {
				child.swap_xy();
			}
		}
	}

	pub fn remove_bbox(&mut self, (x_off, y_off): (u64, u64), size: u64, bbox: &BBox) {
		let ix_min = bbox.x_min.max(x_off);
		let iy_min = bbox.y_min.max(y_off);
		let ix_max = bbox.x_max.min(x_off + size);
		let iy_max = bbox.y_max.min(y_off + size);

		if ix_min >= ix_max || iy_min >= iy_max {
			return;
		}

		if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
			*self = Node::Empty;
			return;
		}

		if self == &Node::Empty {
			return;
		}

		if self == &Node::Full {
			if size == 1 {
				*self = Node::Empty;
				return;
			}
			*self = Node::new_partial_full();
		}

		if let Node::Partial(children) = self {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			children[0].remove_bbox((x_off, y_off), half, bbox);
			children[1].remove_bbox((mid_x, y_off), half, bbox);
			children[2].remove_bbox((x_off, mid_y), half, bbox);
			children[3].remove_bbox((mid_x, mid_y), half, bbox);
			self.normalize();
		}
	}

	pub fn remove_coord(&mut self, (x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) {
		if self == &Node::Empty {
			return;
		}
		if self == &Node::Full {
			if size == 1 {
				*self = Node::Empty;
				return;
			}
			*self = Node::new_partial_full();
		}
		if let Node::Partial(children) = self {
			let (idx, cx, cy, half) = Node::child_quadrant((x_off, y_off), size, (tx, ty));
			children[idx].remove_coord((cx, cy), half, (tx, ty));
			self.normalize();
		}
	}
}
