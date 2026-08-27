use anyhow::{Result, bail};

use crate::TileCover;

/// The two facts [`ensure_same_level`](Self::ensure_same_level) needs from any
/// single-level coverage type, so it can report a mismatch without knowing
/// which of the three it was handed.
///
/// Each implementor already has an inherent `level()` returning the same value,
/// and the impls below forward to it. The names therefore coincide, which is
/// deliberate: on a concrete `TileBBox`, `TileCover` or `TileQuadtree` the
/// inherent method wins, and this trait method is reached only through
/// `&dyn TileCoverInfo` — as `ensure_same_level` does with `other`.
pub trait TileCoverInfo {
	/// The zoom level this coverage lives on.
	fn level(&self) -> u8;

	/// The implementor's name, for error messages.
	fn type_name(&self) -> &'static str;

	/// Errors when `self` and `other` are on different zoom levels, naming both
	/// types and both levels.
	fn ensure_same_level(&self, other: &dyn TileCoverInfo, action: &str) -> Result<()> {
		if self.level() != other.level() {
			bail!(
				"Cannot {} {} with level={} with {} with level={}",
				action,
				self.type_name(),
				self.level(),
				other.type_name(),
				other.level()
			);
		}
		Ok(())
	}
}

impl TileCoverInfo for TileCover {
	fn level(&self) -> u8 {
		TileCover::level(self)
	}

	fn type_name(&self) -> &'static str {
		"TileCover"
	}
}
