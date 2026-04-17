//! Iteration methods for [`TileQuadtree`].

use super::{Node, TileQuadtree};
use crate::TileCoord;

impl TileQuadtree {
	/// Returns an iterator over every tile coordinate in this quadtree.
	///
	/// Coordinates are yielded in depth-first order: NW subtree first, then NE, SW, SE.
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
		TileIter::new(&self.root, 0, 0, self.level)
	}

	/// Splits the quadtree into an aligned grid of sub-quadtrees.
	///
	/// Each cell covers a `size × size` tile block.
	/// Every returned [`TileQuadtree`] has the same `level` as `self`, so their coordinates are directly comparable and their
	/// union equals `self`. Empty grid cells are omitted.
	///
	/// This is analogous to [`crate::TileBBox::iter_grid`].
	pub fn iter_grid(&self, size: u32) -> impl Iterator<Item = TileQuadtree> {
		assert!(size.is_power_of_two(), "grid size must be a power of 2");
		if u64::from(size) >= 1u64 << self.level {
			// If the grid size is larger than the whole quadtree, just return self.
			return vec![self.clone()].into_iter();
		}

		let cell_level = u8::try_from(size.ilog2()).unwrap();
		let depth = self.level - cell_level;
		let level = self.level;
		let mut cells: Vec<TileQuadtree> = Vec::new();
		collect_grid_cells(&self.root, depth, 0, 0, depth, level, &mut cells);
		cells.into_iter()
	}
}

/// DFS iterator that expands quadtree nodes into individual [`TileCoord`]s.
struct TileIter<'a> {
	stack: Vec<IterFrame<'a>>,
	zoom: u8,
}

/// One entry on the [`TileIter`] stack, tracking the node and the tile-space
/// rectangle it covers: `[x_off, x_off+size) × [y_off, y_off+size)`.
struct IterFrame<'a> {
	node: &'a Node,
	x_off: u64,
	y_off: u64,
	size: u64,
}

impl<'a> TileIter<'a> {
	fn new(node: &'a Node, x_off: u64, y_off: u64, level: u8) -> Self {
		let size = 1u64 << level;
		TileIter {
			stack: vec![IterFrame {
				node,
				x_off,
				y_off,
				size,
			}],
			zoom: level,
		}
	}
}

impl Iterator for TileIter<'_> {
	type Item = TileCoord;

	fn next(&mut self) -> Option<TileCoord> {
		loop {
			let frame = self.stack.pop()?;
			match frame.node {
				Node::Empty => (), // skip
				Node::Full => {
					if frame.size == 1 {
						return Some(
							TileCoord::new(
								self.zoom,
								u32::try_from(frame.x_off).unwrap(),
								u32::try_from(frame.y_off).unwrap(),
							)
							.unwrap(),
						);
					}
					// Expand Full node into 4 children (push in reverse for NW-first order)
					let half = frame.size / 2;
					let mid_x = frame.x_off + half;
					let mid_y = frame.y_off + half;
					// Push SE, SW, NE, NW (reversed so NW pops first)
					self.stack.push(IterFrame {
						node: &Node::Full,
						x_off: mid_x,
						y_off: mid_y,
						size: half,
					});
					self.stack.push(IterFrame {
						node: &Node::Full,
						x_off: frame.x_off,
						y_off: mid_y,
						size: half,
					});
					self.stack.push(IterFrame {
						node: &Node::Full,
						x_off: mid_x,
						y_off: frame.y_off,
						size: half,
					});
					self.stack.push(IterFrame {
						node: &Node::Full,
						x_off: frame.x_off,
						y_off: frame.y_off,
						size: half,
					});
				}
				Node::Partial(children) => {
					let half = frame.size / 2;
					let mid_x = frame.x_off + half;
					let mid_y = frame.y_off + half;
					// Push SE, SW, NE, NW reversed
					self.stack.push(IterFrame {
						node: &children[3],
						x_off: mid_x,
						y_off: mid_y,
						size: half,
					});
					self.stack.push(IterFrame {
						node: &children[2],
						x_off: frame.x_off,
						y_off: mid_y,
						size: half,
					});
					self.stack.push(IterFrame {
						node: &children[1],
						x_off: mid_x,
						y_off: frame.y_off,
						size: half,
					});
					self.stack.push(IterFrame {
						node: &children[0],
						x_off: frame.x_off,
						y_off: frame.y_off,
						size: half,
					});
				}
			}
		}
	}
}

/// Recursively walks the tree, collecting one [`TileQuadtree`] per non-empty
/// grid cell into `out`.
///
/// - `remaining` — levels still to descend before we reach a grid-cell leaf.
/// - `(col, row)` — grid-cell coordinates of the current node (in units of `size`).
/// - `depth` — total descent distance from root to leaf, equal to `level - log2(size)`.
///   Passed unchanged to [`embed_node`].
/// - `level` — zoom level of the original tree; all returned trees inherit it.
fn collect_grid_cells(
	node: &Node,
	remaining: u8,
	col: u64,
	row: u64,
	depth: u8,
	level: u8,
	out: &mut Vec<TileQuadtree>,
) {
	if remaining == 0 {
		if !node.is_empty() {
			out.push(TileQuadtree {
				level,
				root: embed_node(node.clone(), col, row, depth),
			});
		}
		return;
	}
	match node {
		Node::Empty => {}
		Node::Full => {
			for (dc, dr) in [(0u64, 0u64), (1, 0), (0, 1), (1, 1)] {
				collect_grid_cells(
					&Node::Full,
					remaining - 1,
					col * 2 + dc,
					row * 2 + dr,
					depth,
					level,
					out,
				);
			}
		}
		Node::Partial(children) => {
			// children order: NW=0, NE=1, SW=2, SE=3
			collect_grid_cells(&children[0], remaining - 1, col * 2, row * 2, depth, level, out);
			collect_grid_cells(&children[1], remaining - 1, col * 2 + 1, row * 2, depth, level, out);
			collect_grid_cells(&children[2], remaining - 1, col * 2, row * 2 + 1, depth, level, out);
			collect_grid_cells(&children[3], remaining - 1, col * 2 + 1, row * 2 + 1, depth, level, out);
		}
	}
}

/// Places `node` at grid position `(col, row)` inside a fresh root of depth `depth`.
///
/// Builds the wrapper from the inside out: at each bit (starting at bit 0) the
/// current subtree is placed into the correct NW/NE/SW/SE child slot determined
/// by the corresponding bits of `col` and `row`.  The root covers the full
/// `[0, 2^depth)^2` tile space with content only in the `(col, row)` cell.
#[allow(clippy::cast_possible_truncation)]
fn embed_node(node: Node, col: u64, row: u64, depth: u8) -> Node {
	let mut result = node;
	for bit in 0..depth {
		let idx = (((col >> bit) & 1) | (((row >> bit) & 1) << 1)) as usize; // NW=0, NE=1, SW=2, SE=3
		let mut children = [Node::Empty, Node::Empty, Node::Empty, Node::Empty];
		children[idx] = result;
		result = Node::new_partial(children);
	}
	result
}
