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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileBBox;
	use anyhow::Result;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn iter_tiles_count() -> Result<()> {
		let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		let tiles: Vec<_> = t.iter_coords().collect();
		assert_eq!(tiles.len() as u64, t.count_tiles());
		assert_eq!(tiles.len(), 16);
		Ok(())
	}

	#[test]
	fn iter_tiles_full() {
		let t = TileQuadtree::new_full(2).unwrap();
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
		let t = TileQuadtree::new_empty(3).unwrap();
		assert_eq!(t.iter_coords().count(), 0);
	}

	#[test]
	fn iter_grid_covers_all() -> Result<()> {
		let t = TileQuadtree::from_bbox(&bbox(4, 0, 0, 15, 15));
		let mut seen = std::collections::HashSet::new();
		let mut total = 0u64;
		for cell in t.iter_grid(4) {
			assert_eq!(cell.level(), 4, "returned cell must have same level as original");
			for c in cell.iter_coords() {
				assert!(seen.insert(c), "duplicate coord {c:?}");
			}
			total += cell.count_tiles();
		}
		assert_eq!(total, t.count_tiles());
		Ok(())
	}

	#[test]
	fn iter_grid_non_aligned_tree() -> Result<()> {
		// Tree covers a non-aligned region; tiles must be partitioned without loss or duplication.
		let t = TileQuadtree::from_bbox(&bbox(4, 1, 1, 6, 6));
		let mut seen = std::collections::HashSet::new();
		let mut total = 0u64;
		for cell in t.iter_grid(4) {
			assert_eq!(cell.level(), 4);
			for c in cell.iter_coords() {
				assert!(seen.insert(c), "duplicate coord {c:?}");
			}
			total += cell.count_tiles();
		}
		assert_eq!(total, t.count_tiles());
		Ok(())
	}

	#[test]
	fn iter_grid_size_equals_full_level() -> Result<()> {
		// size == 2^level → single cell returned containing the whole tree.
		let t = TileQuadtree::from_bbox(&bbox(3, 2, 2, 5, 5));
		let cells: Vec<_> = t.iter_grid(8).collect();
		assert_eq!(cells.len(), 1);
		assert_eq!(cells[0].level(), 3);
		assert_eq!(cells[0].count_tiles(), t.count_tiles());
		// Coords must match exactly
		let orig: std::collections::HashSet<_> = t.iter_coords().collect();
		let cell: std::collections::HashSet<_> = cells[0].iter_coords().collect();
		assert_eq!(orig, cell);
		Ok(())
	}

	#[test]
	fn iter_grid_empty_tree_yields_nothing() {
		let t = TileQuadtree::new_empty(4).unwrap();
		assert_eq!(t.iter_grid(4).count(), 0);
	}
}
