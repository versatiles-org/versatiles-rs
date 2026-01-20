use crate::{TileBBox, TileCoord};

/// Iterator that yields tile coordinates in Z-order (Morton curve) sequence.
///
/// Z-order provides excellent spatial locality by recursively subdividing
/// into quadrants: (0,0), (1,0), (0,1), (1,1). This improves cache hit rates
/// and I/O performance when tiles are stored in spatial order.
///
/// # Memory
///
/// Uses O(level) stack space, not O(n) like collect-and-sort approaches.
pub struct ZOrderIterator {
	level: u8,
	/// Stack of (quadrant_x, quadrant_y, quadrant_size)
	stack: Vec<(u32, u32, u32)>,
	/// Bbox bounds for intersection testing
	x_min: u32,
	y_min: u32,
	x_max: u32,
	y_max: u32,
}

impl ZOrderIterator {
	fn new(bbox: &TileBBox) -> Self {
		if bbox.is_empty() {
			return Self {
				level: bbox.level,
				stack: Vec::new(),
				x_min: 0,
				y_min: 0,
				x_max: 0,
				y_max: 0,
			};
		}

		let size = 1u32 << bbox.level;
		Self {
			level: bbox.level,
			stack: vec![(0, 0, size)],
			x_min: bbox.x_min().unwrap(),
			y_min: bbox.y_min().unwrap(),
			x_max: bbox.x_max().unwrap(),
			y_max: bbox.y_max().unwrap(),
		}
	}

	#[inline]
	fn intersects(&self, qx: u32, qy: u32, size: u32) -> bool {
		qx <= self.x_max && qx + size > self.x_min && qy <= self.y_max && qy + size > self.y_min
	}
}

impl Iterator for ZOrderIterator {
	type Item = TileCoord;

	fn next(&mut self) -> Option<Self::Item> {
		while let Some((qx, qy, size)) = self.stack.pop() {
			if size == 1 {
				// Leaf - emit coordinate (already validated by intersection check)
				return Some(TileCoord::new(self.level, qx, qy).unwrap());
			}

			// Subdivide into 4 quadrants, push in reverse Z-order
			// Z-order: (0,0), (1,0), (0,1), (1,1) → push in reverse: (1,1), (0,1), (1,0), (0,0)
			let half = size / 2;
			for (dx, dy) in [(1, 1), (0, 1), (1, 0), (0, 0)] {
				let sub_x = qx + dx * half;
				let sub_y = qy + dy * half;
				if self.intersects(sub_x, sub_y, half) {
					self.stack.push((sub_x, sub_y, half));
				}
			}
		}
		None
	}
}

// Safe because all fields are Send (u8, Vec, u32)
unsafe impl Send for ZOrderIterator {}

impl TileBBox {
	/// Returns an iterator over all tile coordinates in Z-order (Morton curve).
	///
	/// Z-order provides excellent spatial locality, making it ideal for:
	/// - Tile caching with better cache hit rates
	/// - I/O optimization when tiles are stored spatially
	/// - Parallel processing with reduced contention
	///
	/// # Returns
	///
	/// An iterator yielding `TileCoord` instances in Z-order sequence.
	///
	/// # Example
	///
	/// ```
	/// use versatiles_core::TileBBox;
	///
	/// let bbox = TileBBox::from_min_and_max(2, 0, 0, 1, 1).unwrap();
	/// let coords: Vec<_> = bbox.iter_coords_zorder().collect();
	/// // Z-order: (0,0), (1,0), (0,1), (1,1)
	/// assert_eq!(coords.len(), 4);
	/// ```
	pub fn iter_coords_zorder(&self) -> ZOrderIterator {
		ZOrderIterator::new(self)
	}

	/// Consumes the bounding box and returns an iterator in Z-order (Morton curve).
	///
	/// This version returns `Box<dyn Iterator + Send>` for use with
	/// parallel processing frameworks.
	///
	/// # Returns
	///
	/// A boxed iterator yielding `TileCoord` instances in Z-order sequence.
	pub fn into_iter_coords_zorder(self) -> Box<dyn Iterator<Item = TileCoord> + Send> {
		Box::new(ZOrderIterator::new(&self))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use rstest::rstest;

	fn extract_tc(iter: ZOrderIterator) -> Vec<String> {
		let mut result: [[Option<usize>; 8]; 8] = [[None; 8]; 8];
		for (index, coord) in iter.enumerate() {
			assert_eq!(coord.level, 3);
			result[coord.y as usize][coord.x as usize] = Some(index);
		}
		result
			.into_iter()
			.map(|row| {
				row.into_iter()
					.map(|opt| match opt {
						Some(i) => format!("{i:02}"),
						None => "··".to_string(),
					})
					.collect::<Vec<_>>()
					.join(" ")
			})
			.collect()
	}

	// ------------------------------
	// iter_coords_zorder / into_iter_coords_zorder
	// ------------------------------

	#[test]
	fn zorder_iterator_empty_bbox() -> Result<()> {
		let bb = TileBBox::new_empty(5)?;
		let coords: Vec<_> = bb.iter_coords_zorder().collect();
		assert!(coords.is_empty());
		Ok(())
	}

	#[test]
	fn zorder_iterator_single_tile() {
		let bb = TileBBox::from_min_and_max(3, 5, 3, 5, 3).unwrap();
		assert_eq!(
			extract_tc(bb.iter_coords_zorder()),
			&[
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· 00 ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··"
			]
		);
	}

	#[test]
	fn zorder_iterator_1() {
		let bb = TileBBox::from_min_and_max(3, 0, 0, 2, 2).unwrap();
		assert_eq!(
			extract_tc(bb.iter_coords_zorder()),
			&[
				"00 01 04 ·· ·· ·· ·· ··",
				"02 03 05 ·· ·· ·· ·· ··",
				"06 07 08 ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··"
			]
		);
	}

	#[test]
	fn zorder_iterator_2() {
		let bb = TileBBox::from_min_and_max(3, 0, 1, 2, 3).unwrap();
		assert_eq!(
			extract_tc(bb.iter_coords_zorder()),
			&[
				"·· ·· ·· ·· ·· ·· ·· ··",
				"00 01 02 ·· ·· ·· ·· ··",
				"03 04 07 ·· ·· ·· ·· ··",
				"05 06 08 ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··"
			]
		);
	}

	#[test]
	fn zorder_iterator_3() {
		let bb = TileBBox::from_min_and_max(3, 1, 0, 3, 2).unwrap();
		assert_eq!(
			extract_tc(bb.iter_coords_zorder()),
			&[
				"·· 00 02 03 ·· ·· ·· ··",
				"·· 01 04 05 ·· ·· ·· ··",
				"·· 06 07 08 ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··"
			]
		);
	}

	#[test]
	fn zorder_iterator_offset_bbox_3() {
		let bb = TileBBox::from_min_and_max(3, 1, 1, 4, 4).unwrap();
		assert_eq!(
			extract_tc(bb.iter_coords_zorder()),
			&[
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· 00 01 02 09 ·· ·· ··",
				"·· 03 05 06 10 ·· ·· ··",
				"·· 04 07 08 11 ·· ·· ··",
				"·· 12 13 14 15 ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··",
				"·· ·· ·· ·· ·· ·· ·· ··"
			]
		);
	}

	#[rstest]
	#[case(3, 0, 0, 7, 7)]
	#[case(4, 2, 5, 4, 6)]
	#[case(5, 10, 20, 15, 25)]
	fn zorder_count_matches_rowmajor(#[case] z: u8, #[case] x0: u32, #[case] y0: u32, #[case] x1: u32, #[case] y1: u32) {
		let bb = TileBBox::from_min_and_max(z, x0, y0, x1, y1).unwrap();

		let mut zorder: Vec<_> = bb.iter_coords_zorder().collect();
		let mut rowmajor: Vec<_> = bb.iter_coords().collect();

		assert_eq!(
			zorder.len() as u64,
			bb.count_tiles(),
			"count mismatch for bbox ({x0},{y0})-({x1},{y1}) at z{z}"
		);

		zorder.sort_by_key(|c| (c.y, c.x));
		rowmajor.sort_by_key(|c| (c.y, c.x));
		assert_eq!(zorder, rowmajor);
	}

	#[rstest]
	#[case(3, 0, 0, 7, 7)]
	#[case(4, 2, 5, 4, 6)]
	#[case(5, 10, 20, 15, 25)]
	fn into_iter_coords_zorder_matches(
		#[case] z: u8,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
	) -> Result<()> {
		let bb = TileBBox::from_min_and_max(z, x0, y0, x1, y1)?;
		let a: Vec<_> = bb.iter_coords_zorder().collect();
		let b: Vec<_> = bb.into_iter_coords_zorder().collect();
		assert_eq!(a, b);
		Ok(())
	}

	#[test]
	fn zorder_iterator_is_send() {
		fn assert_send<T: Send>() {}
		assert_send::<super::ZOrderIterator>();
	}
}
