use super::BBox;

/// A single node in the quadtree.
///
/// - `Empty` — no tiles covered in this subtree.
/// - `Full`  — all tiles covered in this subtree.
/// - `Partial` — some tiles covered; children are `[NW, NE, SW, SE]`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
	Empty,
	Full,
	Partial(Box<[Node; 4]>),
}

impl Node {
	/// Normalize: collapse `Partial` where all children are the same.
	pub(crate) fn normalize(&mut self) {
		if let Node::Partial(children) = self {
			if children.iter().all(|c| matches!(c, Node::Full)) {
				*self = Node::Full;
			} else if children.iter().all(|c| matches!(c, Node::Empty)) {
				*self = Node::Empty;
			}
		}
	}

	pub(super) fn new_partial(children: [Node; 4]) -> Node {
		let mut node = Node::Partial(Box::new(children));
		node.normalize();
		node
	}

	pub(super) fn new_partial_empty() -> Node {
		Node::Partial(Box::new([Node::Empty, Node::Empty, Node::Empty, Node::Empty]))
	}

	pub(super) fn new_partial_full() -> Node {
		Node::Partial(Box::new([Node::Full, Node::Full, Node::Full, Node::Full]))
	}

	/// Return true if all tiles in this node's subtree are covered.
	#[must_use]
	pub(crate) fn is_full(&self) -> bool {
		matches!(self, Node::Full)
	}

	/// Return true if no tiles in this node's subtree are covered.
	#[must_use]
	pub(crate) fn is_empty(&self) -> bool {
		matches!(self, Node::Empty)
	}

	pub fn count_tiles(&self, remaining_depth: u8) -> u64 {
		match self {
			Node::Empty => 0,
			Node::Full => 1u64 << (2 * u32::from(remaining_depth)),
			Node::Partial(children) => {
				if remaining_depth == 0 {
					// Shouldn't happen in a well-formed tree, but handle gracefully
					1
				} else {
					children.iter().map(|c| c.count_tiles(remaining_depth - 1)).sum()
				}
			}
		}
	}

	pub fn count_nodes(&self) -> u64 {
		match self {
			Node::Empty | Node::Full => 1,
			Node::Partial(children) => 1 + children.iter().map(Node::count_nodes).sum::<u64>(),
		}
	}

	/// Returns the bounding box `(x_min, y_min, x_max_excl, y_max_excl)` of non-empty tiles.
	pub fn bounds(&self, (x_off, y_off): (u64, u64), size: u64) -> Option<(u64, u64, u64, u64)> {
		match self {
			Node::Empty => None,
			Node::Full => Some((x_off, y_off, x_off + size, y_off + size)),
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				let mut result: Option<(u64, u64, u64, u64)> = None;
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					if let Some(b) = child.bounds((cx, cy), half) {
						result = Some(match result {
							None => b,
							Some(r) => (r.0.min(b.0), r.1.min(b.1), r.2.max(b.2), r.3.max(b.3)),
						});
					}
				}
				result
			}
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

	pub fn includes_coord(&self, (x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let (idx, cx, cy, half) = child_quadrant((x_off, y_off), size, (tx, ty));
				children[idx].includes_coord((cx, cy), half, (tx, ty))
			}
		}
	}

	pub fn includes_bbox(&self, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					let cx_max = cx + half;
					let cy_max = cy + half;
					// Clip bbox against this child's region
					let ix_min = bbox.x_min.max(cx);
					let iy_min = bbox.y_min.max(cy);
					let ix_max = bbox.x_max.min(cx_max);
					let iy_max = bbox.y_max.min(cy_max);
					if ix_min < ix_max && iy_min < iy_max {
						// Pass the clipped sub-bbox so children don't re-clip unnecessarily
						let child_bbox = BBox {
							x_min: ix_min,
							y_min: iy_min,
							x_max: ix_max,
							y_max: iy_max,
						};
						if !child.includes_bbox(cx, cy, half, child_bbox) {
							return false;
						}
					}
				}
				true
			}
		}
	}

	pub fn intersects_tree(&self, b: &Node) -> bool {
		match (self, b) {
			(Node::Empty, _) | (_, Node::Empty) => false,
			(Node::Full, _) | (_, Node::Full) => true,
			(Node::Partial(ac), Node::Partial(bc)) => ac.iter().zip(bc.iter()).any(|(ac, bc)| ac.intersects_tree(bc)),
		}
	}

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
				let (idx, cx, cy, half) = child_quadrant((x_off, y_off), size, (tx, ty));
				children[idx].insert_coord((cx, cy), half, (tx, ty));
				self.normalize();
			}
		}
	}

	pub fn include_bbox(&mut self, (x_off, y_off): (u64, u64), size: u64, bbox: &BBox) {
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
			children[0].include_bbox((x_off, y_off), half, bbox);
			children[1].include_bbox((mid_x, y_off), half, bbox);
			children[2].include_bbox((x_off, mid_y), half, bbox);
			children[3].include_bbox((mid_x, mid_y), half, bbox);
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
			let (idx, cx, cy, half) = child_quadrant((x_off, y_off), size, (tx, ty));
			children[idx].remove_coord((cx, cy), half, (tx, ty));
			self.normalize();
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
}

/// Determine which child quadrant contains `(tx, ty)` and return
/// `(child_index, child_x_off, child_y_off, half_size)`.
///
/// Child indices follow `[NW, NE, SW, SE]` order (index 0..3).
pub(crate) fn child_quadrant((x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) -> (usize, u64, u64, u64) {
	let half = size / 2;
	let mid_x = x_off + half;
	let mid_y = y_off + half;
	if tx < mid_x {
		if ty < mid_y {
			(0, x_off, y_off, half)
		} else {
			(2, x_off, mid_y, half)
		}
	} else if ty < mid_y {
		(1, mid_x, y_off, half)
	} else {
		(3, mid_x, mid_y, half)
	}
}
