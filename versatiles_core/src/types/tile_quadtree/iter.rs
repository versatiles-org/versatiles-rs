//! Iteration methods for [`TileQuadtree`].

use super::{Node, TileQuadtree};
use crate::TileCoord;

impl TileQuadtree {
	/// Iterate over all tile coordinates covered by this quadtree.
	///
	/// Tiles are yielded in DFS order (NW first, then NE, SW, SE).
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
		TileIter::new(&self.root, 0, 0, self.level)
	}

	/// Split the quadtree into a grid of sub-quadtrees, each covering at most
	/// `size × size` tiles at the current zoom level.
	///
	/// This is analogous to `TileBBox::iter_bbox_grid`.
	pub fn iter_bbox_grid(&self, size: u32) -> impl Iterator<Item = TileQuadtree> + '_ {
		assert!(size > 0, "grid size must be > 0");
		let zoom = self.level;
		let total = 1u64 << zoom;
		let s = u64::from(size);
		let cols = total.div_ceil(s);
		let rows = total.div_ceil(s);

		(0..rows).flat_map(move |row| {
			(0..cols).filter_map(move |col| {
				let x_min = col * s;
				let y_min = row * s;
				let x_max = (x_min + s).min(total);
				let y_max = (y_min + s).min(total);

				// Build a sub-quadtree by intersecting self with this grid cell
				use crate::TileBBox;
				let bbox = TileBBox::from_min_and_max(
					zoom,
					u32::try_from(x_min).unwrap(),
					u32::try_from(y_min).unwrap(),
					u32::try_from(x_max - 1).unwrap(),
					u32::try_from(y_max - 1).unwrap(),
				)
				.ok()?;
				let cell_tree = TileQuadtree::from_bbox(&bbox);
				let result = self.intersection(&cell_tree).ok()?;
				if result.is_empty() { None } else { Some(result) }
			})
		})
	}
}

struct TileIter<'a> {
	stack: Vec<IterFrame<'a>>,
	zoom: u8,
}

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
