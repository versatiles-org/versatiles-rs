//! Display, Debug, and PartialEq for [`TileCover`].

use super::TileCover;
use std::fmt;

impl fmt::Display for TileCover {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			TileCover::Bbox(b) => write!(f, "{b}"),
			TileCover::Tree(t) => write!(f, "{t}"),
		}
	}
}

impl fmt::Debug for TileCover {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			TileCover::Bbox(b) => write!(f, "TileCover::Bbox({b:?})"),
			TileCover::Tree(t) => write!(f, "TileCover::Tree({t})"),
		}
	}
}

impl PartialEq for TileCover {
	fn eq(&self, other: &Self) -> bool {
		if self.level() != other.level() {
			return false;
		}
		match (self, other) {
			(TileCover::Bbox(a), TileCover::Bbox(b)) => a == b,
			(TileCover::Tree(a), TileCover::Tree(b)) => a == b,
			_ => {
				if self.count_tiles() != other.count_tiles() {
					return false;
				}
				self.bounds() == other.bounds()
			}
		}
	}
}
