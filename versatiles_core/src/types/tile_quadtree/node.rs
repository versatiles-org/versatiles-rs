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

	/// Determine which child quadrant contains `(tx, ty)` and return
	/// `(child_index, child_x_off, child_y_off, half_size)`.
	///
	/// Child indices follow `[NW, NE, SW, SE]` order (index 0..3).
	pub(super) fn child_quadrant((x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) -> (usize, u64, u64, u64) {
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
}
