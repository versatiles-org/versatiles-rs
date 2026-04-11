//! Mutation methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, Node, TileQuadtree, child_quadrant};
use crate::{TileBBox, TileCoord};
use anyhow::Result;

impl TileQuadtree {
	/// Insert a single tile into the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	pub fn insert_tile(&mut self, coord: TileCoord) -> Result<()> {
		check_coord_zoom(coord, self.zoom)?;
		let size = 1u64 << self.zoom;
		let new_root = node_insert_tile(
			std::mem::replace(&mut self.root, Node::Empty),
			0,
			0,
			size,
			u64::from(coord.x),
			u64::from(coord.y),
		);
		self.root = new_root;
		Ok(())
	}

	/// Insert all tiles within a [`TileBBox`] into the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn insert_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.zoom)?;
		if bbox.is_empty() {
			return Ok(());
		}
		let size = 1u64 << self.zoom;
		let bx_min = u64::from(bbox.x_min()?);
		let by_min = u64::from(bbox.y_min()?);
		let bx_max = u64::from(bbox.x_max()?) + 1;
		let by_max = u64::from(bbox.y_max()?) + 1;
		let new_root = node_insert_bbox(
			std::mem::replace(&mut self.root, Node::Empty),
			0,
			0,
			size,
			BBox {
				x_min: bx_min,
				y_min: by_min,
				x_max: bx_max,
				y_max: by_max,
			},
		);
		self.root = new_root;
		Ok(())
	}

	/// Remove a single tile from the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	pub fn remove_tile(&mut self, coord: TileCoord) -> Result<()> {
		check_coord_zoom(coord, self.zoom)?;
		let size = 1u64 << self.zoom;
		let new_root = node_remove_tile(
			std::mem::replace(&mut self.root, Node::Empty),
			0,
			0,
			size,
			u64::from(coord.x),
			u64::from(coord.y),
		);
		self.root = new_root;
		Ok(())
	}

	/// Remove all tiles within a [`TileBBox`] from the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.zoom)?;
		if bbox.is_empty() {
			return Ok(());
		}
		let size = 1u64 << self.zoom;
		let bx_min = u64::from(bbox.x_min()?);
		let by_min = u64::from(bbox.y_min()?);
		let bx_max = u64::from(bbox.x_max()?) + 1;
		let by_max = u64::from(bbox.y_max()?) + 1;
		let new_root = node_remove_bbox(
			std::mem::replace(&mut self.root, Node::Empty),
			0,
			0,
			size,
			BBox {
				x_min: bx_min,
				y_min: by_min,
				x_max: bx_max,
				y_max: by_max,
			},
		);
		self.root = new_root;
		Ok(())
	}
}

fn node_insert_tile(node: Node, x_off: u64, y_off: u64, size: u64, tx: u64, ty: u64) -> Node {
	match node {
		Node::Full => Node::Full,
		Node::Empty => {
			if size == 1 {
				Node::Full
			} else {
				// Expand to Partial and recurse
				let mut children = [Node::Empty, Node::Empty, Node::Empty, Node::Empty];
				let (idx, cx, cy, half) = child_quadrant(x_off, y_off, size, tx, ty);
				children[idx] = node_insert_tile(Node::Empty, cx, cy, half, tx, ty);
				Node::normalize(children)
			}
		}
		Node::Partial(mut children) => {
			let (idx, cx, cy, half) = child_quadrant(x_off, y_off, size, tx, ty);
			let child = std::mem::replace(&mut children[idx], Node::Empty);
			children[idx] = node_insert_tile(child, cx, cy, half, tx, ty);
			Node::normalize(*children)
		}
	}
}

fn node_insert_bbox(node: Node, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> Node {
	// Intersection of bbox with this cell
	let ix_min = bbox.x_min.max(x_off);
	let iy_min = bbox.y_min.max(y_off);
	let ix_max = bbox.x_max.min(x_off + size);
	let iy_max = bbox.y_max.min(y_off + size);

	if ix_min >= ix_max || iy_min >= iy_max {
		return node; // bbox doesn't touch this cell
	}

	// If bbox covers the full cell, mark Full
	if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
		return Node::Full;
	}

	match node {
		Node::Full => Node::Full,
		Node::Empty => {
			if size == 1 {
				Node::Full
			} else {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let children = [
					node_insert_bbox(Node::Empty, x_off, y_off, half, bbox),
					node_insert_bbox(Node::Empty, mid_x, y_off, half, bbox),
					node_insert_bbox(Node::Empty, x_off, mid_y, half, bbox),
					node_insert_bbox(Node::Empty, mid_x, mid_y, half, bbox),
				];
				Node::normalize(children)
			}
		}
		Node::Partial(children) => {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			let [nw, ne, sw, se] = *children;
			let children = [
				node_insert_bbox(nw, x_off, y_off, half, bbox),
				node_insert_bbox(ne, mid_x, y_off, half, bbox),
				node_insert_bbox(sw, x_off, mid_y, half, bbox),
				node_insert_bbox(se, mid_x, mid_y, half, bbox),
			];
			Node::normalize(children)
		}
	}
}

fn node_remove_tile(node: Node, x_off: u64, y_off: u64, size: u64, tx: u64, ty: u64) -> Node {
	match node {
		Node::Empty => Node::Empty,
		Node::Full => {
			if size == 1 {
				Node::Empty
			} else {
				// Expand to Partial([Full; 4]) and recurse
				let mut children = [Node::Full, Node::Full, Node::Full, Node::Full];
				let (idx, cx, cy, half) = child_quadrant(x_off, y_off, size, tx, ty);
				children[idx] = node_remove_tile(Node::Full, cx, cy, half, tx, ty);
				Node::normalize(children)
			}
		}
		Node::Partial(mut children) => {
			let (idx, cx, cy, half) = child_quadrant(x_off, y_off, size, tx, ty);
			let child = std::mem::replace(&mut children[idx], Node::Empty);
			children[idx] = node_remove_tile(child, cx, cy, half, tx, ty);
			Node::normalize(*children)
		}
	}
}

fn node_remove_bbox(node: Node, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> Node {
	let ix_min = bbox.x_min.max(x_off);
	let iy_min = bbox.y_min.max(y_off);
	let ix_max = bbox.x_max.min(x_off + size);
	let iy_max = bbox.y_max.min(y_off + size);

	if ix_min >= ix_max || iy_min >= iy_max {
		return node;
	}

	if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
		return Node::Empty;
	}

	match node {
		Node::Empty => Node::Empty,
		Node::Full => {
			if size == 1 {
				Node::Empty
			} else {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let children = [
					node_remove_bbox(Node::Full, x_off, y_off, half, bbox),
					node_remove_bbox(Node::Full, mid_x, y_off, half, bbox),
					node_remove_bbox(Node::Full, x_off, mid_y, half, bbox),
					node_remove_bbox(Node::Full, mid_x, mid_y, half, bbox),
				];
				Node::normalize(children)
			}
		}
		Node::Partial(children) => {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			let [nw, ne, sw, se] = *children;
			let children = [
				node_remove_bbox(nw, x_off, y_off, half, bbox),
				node_remove_bbox(ne, mid_x, y_off, half, bbox),
				node_remove_bbox(sw, x_off, mid_y, half, bbox),
				node_remove_bbox(se, mid_x, mid_y, half, bbox),
			];
			Node::normalize(children)
		}
	}
}
