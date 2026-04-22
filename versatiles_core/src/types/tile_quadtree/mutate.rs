//! Mutation methods for [`TileQuadtree`].

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
		self.check_zoom(coord.level)?;
		self
			.root
			.insert_coord(&BBox::root(self.level), (u64::from(coord.x), u64::from(coord.y)));
		Ok(())
	}

	/// Insert all tiles within a [`TileBBox`] into the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	#[context("Failed to include TileBBox {bbox:?} into TileQuadtree at level {}", self.level)]
	pub fn insert_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.check_zoom(bbox.level())?;
		let Some(bbox) = BBox::from_bbox(bbox) else {
			return Ok(());
		};
		self.root.insert_bbox(&BBox::root(self.level), &bbox);
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
		let root_cell = BBox::root(self.level);
		let tree_size = root_cell.size();
		let n = u64::from(size);

		let mut rects: Vec<BBox> = Vec::new();
		self.root.collect_full_rects(&root_cell, &mut rects);

		let mut new_root = Node::Empty;
		for rect in rects {
			let expanded = BBox {
				x_min: rect.x_min.saturating_sub(n),
				y_min: rect.y_min.saturating_sub(n),
				x_max: (rect.x_max + n).min(tree_size),
				y_max: (rect.y_max + n).min(tree_size),
			};
			new_root.insert_bbox(&root_cell, &expanded);
		}
		self.root = new_root;
	}

	/// Remove a single tile from the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	#[context("Failed to remove TileCoord {coord:?} from TileQuadtree at level {}", self.level)]
	pub fn remove_coord(&mut self, coord: &TileCoord) -> Result<()> {
		self.check_zoom(coord.level)?;
		self
			.root
			.remove_coord(&BBox::root(self.level), (u64::from(coord.x), u64::from(coord.y)));
		Ok(())
	}

	/// Remove all tiles within a [`TileBBox`] from the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	#[context("Failed to remove TileBBox {bbox:?} from TileQuadtree at level {}", self.level)]
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.check_zoom(bbox.level())?;
		let Some(bbox) = BBox::from_bbox(bbox) else {
			return Ok(());
		};
		self.root.remove_bbox(&BBox::root(self.level), &bbox);
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
}

impl Node {
	pub fn insert_coord(&mut self, cell: &BBox, (tx, ty): (u64, u64)) {
		match self {
			Node::Full => (),
			Node::Empty => {
				if cell.size() == 1 {
					*self = Node::Full;
				} else {
					*self = Node::new_partial_empty();
					self.insert_coord(cell, (tx, ty));
				}
			}
			Node::Partial(children) => {
				let (idx, child_cell) = Node::child_quadrant(cell, (tx, ty));
				children[idx].insert_coord(&child_cell, (tx, ty));
				self.normalize();
			}
		}
	}

	pub fn insert_bbox(&mut self, cell: &BBox, bbox: &BBox) {
		// bbox doesn't touch this cell.
		if cell.intersection(bbox).is_none() {
			return;
		}
		// bbox covers the full cell.
		if bbox.covers(cell) {
			*self = Node::Full;
			return;
		}
		if self == &Node::Full {
			return;
		}
		if self == &Node::Empty {
			*self = Node::new_partial_empty();
		}
		if let Node::Partial(children) = self {
			let quads = cell.quadrants();
			for (child, q) in children.iter_mut().zip(&quads) {
				child.insert_bbox(q, bbox);
			}
			self.normalize();
		}
	}

	/// Collect the bounding rectangle of every `Full` subtree into `out`.
	///
	/// Each entry uses exclusive upper bounds, matching the internal [`BBox`]
	/// convention used throughout this module.
	pub(crate) fn collect_full_rects(&self, cell: &BBox, out: &mut Vec<super::BBox>) {
		match self {
			Node::Empty => {}
			Node::Full => out.push(*cell),
			Node::Partial(children) => {
				let quads = cell.quadrants();
				for (child, q) in children.iter().zip(&quads) {
					child.collect_full_rects(q, out);
				}
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

	pub fn remove_bbox(&mut self, cell: &BBox, bbox: &BBox) {
		if cell.intersection(bbox).is_none() {
			return;
		}
		if bbox.covers(cell) {
			*self = Node::Empty;
			return;
		}
		if self == &Node::Empty {
			return;
		}
		if self == &Node::Full {
			*self = Node::new_partial_full();
		}
		if let Node::Partial(children) = self {
			let quads = cell.quadrants();
			for (child, q) in children.iter_mut().zip(&quads) {
				child.remove_bbox(q, bbox);
			}
			self.normalize();
		}
	}

	pub fn remove_coord(&mut self, cell: &BBox, (tx, ty): (u64, u64)) {
		if self == &Node::Empty {
			return;
		}
		if self == &Node::Full {
			if cell.size() == 1 {
				*self = Node::Empty;
				return;
			}
			*self = Node::new_partial_full();
		}
		if let Node::Partial(children) = self {
			let (idx, child_cell) = Node::child_quadrant(cell, (tx, ty));
			children[idx].remove_coord(&child_cell, (tx, ty));
			self.normalize();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	// -------------------------------------------------------------------------
	// insert / remove
	// -------------------------------------------------------------------------

	#[test]
	fn insert_coord() -> Result<()> {
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 0, 0))?;
		assert_eq!(t.count_tiles(), 1);
		assert!(t.includes_coord(&coord(3, 0, 0)));
		assert!(!t.includes_coord(&coord(3, 1, 0)));
		Ok(())
	}

	#[test]
	fn insert_tile_collapses_to_full() -> Result<()> {
		// At zoom 1, there are only 4 tiles. Insert all 4.
		let mut t = TileQuadtree::new_empty(1).unwrap();
		t.insert_coord(&coord(1, 0, 0))?;
		t.insert_coord(&coord(1, 1, 0))?;
		t.insert_coord(&coord(1, 0, 1))?;
		t.insert_coord(&coord(1, 1, 1))?;
		assert!(t.is_full());
		Ok(())
	}

	#[test]
	fn insert_bbox() -> Result<()> {
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.insert_bbox(&bbox(4, 0, 0, 7, 7))?;
		assert_eq!(t.count_tiles(), 64);
		Ok(())
	}

	#[test]
	fn remove_coord() -> Result<()> {
		let mut t = TileQuadtree::new_full(2).unwrap();
		t.remove_coord(&coord(2, 0, 0))?;
		assert!(!t.is_full());
		assert_eq!(t.count_tiles(), 15);
		assert!(!t.includes_coord(&coord(2, 0, 0)));
		assert!(t.includes_coord(&coord(2, 1, 0)));
		Ok(())
	}

	#[test]
	fn remove_bbox() -> Result<()> {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.remove_bbox(&bbox(3, 0, 0, 3, 7))?;
		assert!(!t.is_full());
		assert_eq!(t.count_tiles(), 32); // half the tiles removed
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Zoom mismatches
	// -------------------------------------------------------------------------

	#[test]
	fn zoom_mismatch_include_coord() {
		let mut t = TileQuadtree::new_empty(3).unwrap();
		assert!(t.insert_coord(&coord(4, 0, 0)).is_err());
	}

	#[test]
	fn zoom_mismatch_include_bbox() {
		let mut t = TileQuadtree::new_empty(3).unwrap();
		assert!(t.insert_bbox(&bbox(4, 0, 0, 1, 1)).is_err());
	}

	#[test]
	fn zoom_mismatch_remove_coord() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		assert!(t.remove_coord(&coord(4, 0, 0)).is_err());
	}

	#[test]
	fn zoom_mismatch_remove_bbox() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		assert!(t.remove_bbox(&bbox(4, 0, 0, 1, 1)).is_err());
	}

	// -------------------------------------------------------------------------
	// Empty-bbox no-ops
	// -------------------------------------------------------------------------

	#[test]
	fn include_empty_bbox_is_noop() -> Result<()> {
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.insert_bbox(&TileBBox::new_empty(4)?)?;
		assert!(t.is_empty());
		Ok(())
	}

	#[test]
	fn remove_empty_bbox_is_noop() -> Result<()> {
		let mut t = TileQuadtree::new_full(3).unwrap();
		let count_before = t.count_tiles();
		t.remove_bbox(&TileBBox::new_empty(3)?)?;
		assert_eq!(t.count_tiles(), count_before);
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Buffer
	// -------------------------------------------------------------------------

	#[test]
	fn buffer_zero_is_noop() {
		let t = TileQuadtree::from_bbox(&bbox(4, 3, 3, 8, 8));
		let mut t2 = t.clone();
		t2.buffer(0);
		assert_eq!(t2, t);
		assert_eq!(t2.count_tiles(), t.count_tiles());
	}

	#[test]
	fn buffer_empty_is_noop() {
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.buffer(2);
		assert!(t.is_empty());
	}

	#[test]
	fn buffer_full_stays_full() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.buffer(5);
		assert!(t.is_full());
	}

	#[test]
	fn buffer_single_tile_expands_to_square() -> Result<()> {
		// Single tile at (4, 4) at zoom 4; buffer(2) expands to (2..6, 2..6) = 5×5 = 25 tiles
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.insert_coord(&coord(4, 4, 4))?;
		t.buffer(2);
		assert_eq!(t.count_tiles(), 25);
		assert!(t.includes_coord(&coord(4, 2, 2))); // top-left corner added
		assert!(t.includes_coord(&coord(4, 6, 6))); // bottom-right corner added
		assert!(!t.includes_coord(&coord(4, 1, 4))); // just outside
		Ok(())
	}

	#[test]
	fn buffer_clamps_at_boundary() -> Result<()> {
		// Tile at (0, 0); buffer(3) clamped at x=0, y=0 → (0..3, 0..3) = 4×4 = 16 tiles
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.insert_coord(&coord(4, 0, 0))?;
		t.buffer(3);
		assert_eq!(t.count_tiles(), 16);
		assert!(t.includes_coord(&coord(4, 3, 3))); // far corner
		assert!(!t.includes_coord(&coord(4, 4, 0))); // just outside buffer
		Ok(())
	}

	#[test]
	fn buffer_rectangular_region() -> Result<()> {
		// 3×3 block at (2,2)–(4,4); buffer(1) → (1,1)–(5,5) = 5×5 = 25 tiles
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.insert_bbox(&bbox(4, 2, 2, 4, 4))?;
		assert_eq!(t.count_tiles(), 9);
		t.buffer(1);
		assert_eq!(t.count_tiles(), 25);
		Ok(())
	}

	// -------------------------------------------------------------------------
	// flip_y
	// -------------------------------------------------------------------------

	#[test]
	fn flip_y_empty_noop() {
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.flip_y();
		assert!(t.is_empty());
	}

	#[test]
	fn flip_y_full_stays_full() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.flip_y();
		assert!(t.is_full());
		assert_eq!(t.count_tiles(), 64);
	}

	#[test]
	fn flip_y_moves_tile_to_mirrored_position() -> Result<()> {
		// z=3: 8×8 grid; tile (2, 1) should flip to (2, 8−1−1) = (2, 6)
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 2, 1))?;
		t.flip_y();
		assert!(!t.includes_coord(&coord(3, 2, 1)));
		assert!(t.includes_coord(&coord(3, 2, 6)));
		assert_eq!(t.count_tiles(), 1);
		Ok(())
	}

	#[test]
	fn flip_y_is_involution() -> Result<()> {
		// Two applications should return the original tree.
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 2, 1))?;
		t.insert_coord(&coord(3, 5, 6))?;
		let count = t.count_tiles();
		t.flip_y();
		t.flip_y();
		assert!(t.includes_coord(&coord(3, 2, 1)));
		assert!(t.includes_coord(&coord(3, 5, 6)));
		assert_eq!(t.count_tiles(), count);
		Ok(())
	}

	// -------------------------------------------------------------------------
	// swap_xy
	// -------------------------------------------------------------------------

	#[test]
	fn swap_xy_empty_noop() {
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.swap_xy();
		assert!(t.is_empty());
	}

	#[test]
	fn swap_xy_full_stays_full() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.swap_xy();
		assert!(t.is_full());
		assert_eq!(t.count_tiles(), 64);
	}

	#[test]
	fn swap_xy_moves_tile_to_transposed_position() -> Result<()> {
		// z=3: 8×8 grid; tile (2, 5) should swap to (5, 2)
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 2, 5))?;
		t.swap_xy();
		assert!(!t.includes_coord(&coord(3, 2, 5)));
		assert!(t.includes_coord(&coord(3, 5, 2)));
		assert_eq!(t.count_tiles(), 1);
		Ok(())
	}

	#[test]
	fn swap_xy_is_involution() -> Result<()> {
		// Two applications should return the original tree.
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 2, 5))?;
		t.insert_coord(&coord(3, 0, 7))?;
		let count = t.count_tiles();
		t.swap_xy();
		t.swap_xy();
		assert!(t.includes_coord(&coord(3, 2, 5)));
		assert!(t.includes_coord(&coord(3, 0, 7)));
		assert_eq!(t.count_tiles(), count);
		Ok(())
	}

	// ── Zoom mismatches: consolidated via rstest ─────────────────────────────
	use rstest::rstest;

	enum Op {
		InsertCoord,
		RemoveCoord,
		InsertBBox,
		RemoveBBox,
	}

	#[rstest]
	#[case(Op::InsertCoord)]
	#[case(Op::RemoveCoord)]
	#[case(Op::InsertBBox)]
	#[case(Op::RemoveBBox)]
	fn zoom_mismatch_all_mutations(#[case] op: Op) {
		let mut t = TileQuadtree::new_full(3).unwrap();
		let err = match op {
			Op::InsertCoord => t.insert_coord(&coord(4, 0, 0)),
			Op::RemoveCoord => t.remove_coord(&coord(4, 0, 0)),
			Op::InsertBBox => t.insert_bbox(&bbox(4, 0, 0, 1, 1)),
			Op::RemoveBBox => t.remove_bbox(&bbox(4, 0, 0, 1, 1)),
		};
		assert!(err.is_err());
	}

	// ── Collapse-to-Full / Collapse-to-Empty invariants ──────────────────────
	#[test]
	fn insert_all_tiles_at_level_2_collapses_to_full() -> Result<()> {
		let mut t = TileQuadtree::new_empty(2).unwrap();
		for y in 0..4u32 {
			for x in 0..4u32 {
				t.insert_coord(&coord(2, x, y))?;
			}
		}
		assert!(t.is_full());
		// Collapse means one node.
		assert_eq!(t.count_nodes(), 1);
		Ok(())
	}

	#[test]
	fn remove_all_tiles_collapses_to_empty() -> Result<()> {
		let mut t = TileQuadtree::new_full(2).unwrap();
		for y in 0..4u32 {
			for x in 0..4u32 {
				t.remove_coord(&coord(2, x, y))?;
			}
		}
		assert!(t.is_empty());
		assert_eq!(t.count_nodes(), 1);
		Ok(())
	}

	#[test]
	fn insert_single_tile_into_full_is_noop() -> Result<()> {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.insert_coord(&coord(3, 2, 3))?;
		assert!(t.is_full());
		Ok(())
	}

	#[test]
	fn remove_single_tile_from_empty_is_noop() -> Result<()> {
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.remove_coord(&coord(3, 2, 3))?;
		assert!(t.is_empty());
		Ok(())
	}

	// ── buffer identity sweep across buffer sizes ────────────────────────────
	#[rstest]
	#[case(0)]
	#[case(1)]
	#[case(8)]
	fn buffer_empty_stays_empty_for_any_size(#[case] size: u32) {
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.buffer(size);
		assert!(t.is_empty());
	}

	#[rstest]
	#[case(0)]
	#[case(1)]
	#[case(8)]
	fn buffer_full_stays_full_for_any_size(#[case] size: u32) {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.buffer(size);
		assert!(t.is_full());
	}

	// ── flip_y / swap_xy involution on various trees ─────────────────────────
	#[rstest]
	#[case(TileQuadtree::new_empty(3).unwrap())]
	#[case(TileQuadtree::new_full(3).unwrap())]
	#[case(TileQuadtree::from_bbox(&bbox(4, 1, 2, 5, 6)))]
	#[case(TileQuadtree::from_tile_coords(3, &[(0, 0), (7, 7), (2, 5)]).unwrap())]
	fn flip_y_involution(#[case] tree: TileQuadtree) {
		let mut t = tree.clone();
		t.flip_y();
		t.flip_y();
		assert_eq!(t, tree);
	}

	#[rstest]
	#[case(TileQuadtree::new_empty(3).unwrap())]
	#[case(TileQuadtree::new_full(3).unwrap())]
	#[case(TileQuadtree::from_bbox(&bbox(4, 1, 2, 5, 6)))]
	#[case(TileQuadtree::from_tile_coords(3, &[(0, 0), (7, 7), (2, 5)]).unwrap())]
	fn swap_xy_involution(#[case] tree: TileQuadtree) {
		let mut t = tree.clone();
		t.swap_xy();
		t.swap_xy();
		assert_eq!(t, tree);
	}
}
