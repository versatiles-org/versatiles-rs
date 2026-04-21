use super::{BBox, Node};
use crate::{GeoBBox, TileBBox, TileQuadtree};

impl TileQuadtree {
	/// Return the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if the quadtree is empty.
	#[must_use]
	pub fn to_bbox(&self) -> TileBBox {
		self
			.root
			.bounds((0, 0), 1u64 << self.level)
			.map_or_else(|| TileBBox::new_empty(self.level).unwrap(), |b| b.into_bbox(self.level))
	}

	/// Convert the covered area to a geographic [`GeoBBox`], or `None` if empty.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		self.to_bbox().to_geo_bbox()
	}
}

impl Node {
	/// Returns the bounding box `(x_min, y_min, x_max_excl, y_max_excl)` of non-empty tiles.
	pub fn bounds(&self, (x_off, y_off): (u64, u64), size: u64) -> Option<BBox> {
		match self {
			Node::Empty => None,
			Node::Full => Some(BBox::new(x_off, y_off, x_off + size, y_off + size)),
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				let mut result: Option<BBox> = None;
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					if let Some(b) = child.bounds((cx, cy), half) {
						result = Some(match result {
							None => b,
							Some(r) => r.union(b),
						});
					}
				}
				result
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
