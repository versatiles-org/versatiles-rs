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

	/// Creates a `Partial` node from the given four children, normalising to
	/// `Empty` or `Full` when all children are uniform.
	pub(super) fn new_partial(children: [Node; 4]) -> Node {
		let mut node = Node::Partial(Box::new(children));
		node.normalize();
		node
	}

	/// Creates a `Partial` node with all four children set to `Empty`.
	pub(super) fn new_partial_empty() -> Node {
		Node::Partial(Box::new([Node::Empty, Node::Empty, Node::Empty, Node::Empty]))
	}

	/// Creates a `Partial` node with all four children set to `Full`.
	pub(super) fn new_partial_full() -> Node {
		Node::Partial(Box::new([Node::Full, Node::Full, Node::Full, Node::Full]))
	}

	/// Determine which child quadrant contains `(tx, ty)` and return
	/// `(child_index, child_cell)`.
	///
	/// Child indices follow `[NW, NE, SW, SE]` order (index 0..3).
	pub(super) fn child_quadrant(cell: &BBox, (tx, ty): (u64, u64)) -> (usize, BBox) {
		let quads = cell.quadrants();
		let mid_x = quads[0].x_max;
		let mid_y = quads[0].y_max;
		let idx = match (tx < mid_x, ty < mid_y) {
			(true, true) => 0,
			(false, true) => 1,
			(true, false) => 2,
			(false, false) => 3,
		};
		(idx, quads[idx])
	}
}
