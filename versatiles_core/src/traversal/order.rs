use crate::{TileBBox, utils::HilbertIndex};
use anyhow::{Result, bail};
use enumset::EnumSetType;

#[derive(EnumSetType, Debug)]
pub enum TraversalOrder {
	AnyOrder,
	DepthFirst,
	PMTiles,
}

impl TraversalOrder {
	pub fn sort_bboxes(&self, bboxes: Vec<TileBBox>, size: u32) -> Vec<TileBBox> {
		use TraversalOrder::*;
		match self {
			AnyOrder => bboxes,
			DepthFirst => sort_depth_first(bboxes, size),
			PMTiles => sort_hilbert(bboxes),
		}
	}

	pub fn intersect(&mut self, other: &TraversalOrder) -> Result<()> {
		use TraversalOrder::*;
		if self == other || other == &AnyOrder {
			return Ok(());
		}
		if self == &AnyOrder {
			*self = *other;
			return Ok(());
		}
		bail!(
			"Incompatible traversal orders, cannot intersect {:?} with {:?}",
			self,
			other
		);
	}
}

fn sort_depth_first(bboxes: Vec<TileBBox>, size: u32) -> Vec<TileBBox> {
	/// Build a depth‑first post‑order sort key for a chunk at (x_chunk, y_chunk).
	///
	/// The algorithm converts `(x_chunk, y_chunk)` to a quadtree path from the
	/// root down to that chunk and then appends the sentinel digit **4**.
	/// Children therefore have a key beginning with the same prefix but ending
	/// with “…, child_digit, 4”, while the parent ends with “…, 4”.
	/// In lexicographic order the children (`0‥3`) all come **before**
	/// their parent (`4`), which yields the desired “children before parent”
	/// post‑order traversal.
	fn build_key(depth: u8, x: u32, y: u32) -> Vec<u8> {
		let mut k = Vec::with_capacity(depth as usize + 1);
		// Traverse from the root (most‑significant bit) towards the leaves.
		for i in (0..depth).rev() {
			let bit_x = (x >> i) & 1;
			let bit_y = (y >> i) & 1;
			k.push((bit_x | (bit_y << 1)) as u8); // quadrant digit 0‥3
		}
		k.push(4); // sentinel – guarantees parent after its (up‑to‑4) children
		k
	}

	// ---------------------------------------------------------------------
	// 1.  Flatten all incoming bboxes into fixed‑size chunks of `chunk_size`.
	// ---------------------------------------------------------------------
	let mut items: Vec<(Vec<u8>, TileBBox)> = bboxes
		.into_iter()
		.map(|b| (build_key(b.level, b.x_min / size, b.y_min / size), b))
		.collect();

	// ---------------------------------------------------------------------
	// 2.  Single `sort_unstable_by` to obtain the post‑order traversal.
	// ---------------------------------------------------------------------
	items.sort_unstable_by(|a, b| a.0.cmp(&b.0));

	// ---------------------------------------------------------------------
	// 3.  Strip the keys and return the ordered bounding boxes.
	// ---------------------------------------------------------------------
	items.into_iter().map(|(_, bbox)| bbox).collect()
}

fn sort_hilbert(mut bboxes: Vec<TileBBox>) -> Vec<TileBBox> {
	bboxes.sort_by_cached_key(|b| b.get_hilbert_index().unwrap());
	bboxes
}
