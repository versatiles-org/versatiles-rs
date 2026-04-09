//! Display formatting for [`TileQuadtree`].

use super::TileQuadtree;
use std::fmt;

impl fmt::Display for TileQuadtree {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "TileQuadtree(zoom={}, tiles={})", self.zoom, self.tile_count())
	}
}
