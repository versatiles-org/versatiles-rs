//! Display formatting for [`TileQuadtree`].

use super::TileQuadtree;
use std::fmt;

impl fmt::Display for TileQuadtree {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"TileQuadtree(zoom={}, tiles={}, nodes={})",
			self.level,
			self.count_tiles(),
			self.count_nodes()
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn display() {
		let t = TileQuadtree::new_full(3).unwrap();
		let s = format!("{t}");
		assert!(s.contains("zoom=3"));
		assert!(s.contains("tiles=64"));
	}
}
