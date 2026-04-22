use super::{BBox, Node};
use crate::{GeoBBox, TileBBox, TileQuadtree};

impl TileQuadtree {
	/// Return the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if the quadtree is empty.
	#[must_use]
	pub fn to_bbox(&self) -> TileBBox {
		self.root.bounds(&BBox::root(self.level)).map_or_else(
			|| TileBBox::new_empty(self.level).expect("level already validated"),
			|b| b.into_bbox(self.level),
		)
	}

	/// Convert the covered area to a geographic [`GeoBBox`], or `None` if empty.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		self.to_bbox().to_geo_bbox()
	}
}

impl Node {
	/// Returns the bounding box of non-empty tiles within `cell`, or `None` if
	/// this subtree is empty.
	pub fn bounds(&self, cell: &BBox) -> Option<BBox> {
		match self {
			Node::Empty => None,
			Node::Full => Some(*cell),
			Node::Partial(children) => {
				let quads = cell.quadrants();
				children
					.iter()
					.zip(&quads)
					.filter_map(|(child, q)| child.bounds(q))
					.reduce(BBox::union)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn bounds_empty_and_full() -> Result<()> {
		assert!(TileQuadtree::new_empty(3).unwrap().to_bbox().is_empty());
		assert_eq!(TileQuadtree::new_full(2).unwrap().to_bbox(), TileBBox::new_full(2)?);
		Ok(())
	}

	#[test]
	fn bounds_partial() -> Result<()> {
		let b = bbox(4, 3, 5, 7, 9);
		let t = TileQuadtree::from_bbox(&b);
		assert_eq!(t.to_bbox(), b);
		Ok(())
	}

	#[test]
	fn to_geo_bbox_empty_is_none() {
		assert!(TileQuadtree::new_empty(4).unwrap().to_geo_bbox().is_none());
	}

	#[test]
	fn to_geo_bbox_full_covers_world() {
		let geo = TileQuadtree::new_full(0).unwrap().to_geo_bbox().unwrap();
		assert!(geo.x_min <= -179.0);
		assert!(geo.x_max >= 179.0);
	}
}
