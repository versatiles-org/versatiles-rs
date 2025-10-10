use super::compression_goal::CompressionGoal;
use crate::TileCompression;
use enumset::EnumSet;
use std::fmt::{self, Debug};

/// Represents the target compression settings.
#[derive(PartialEq)]
pub struct TargetCompression {
	/// Set of allowed compression algorithms.
	pub compressions: EnumSet<TileCompression>,
	/// Desired compression goal.
	pub compression_goal: CompressionGoal,
}

impl TargetCompression {
	/// Creates a new `TargetCompression` with a set of allowed compressions.
	///
	/// By default, the compression goal is set to `UseBestCompression`.
	///
	/// # Arguments
	///
	/// * `compressions` - A set of allowed compression algorithms.
	///
	/// # Returns
	///
	/// * `TargetCompression` instance.
	#[must_use]
	pub fn from_set(compressions: EnumSet<TileCompression>) -> Self {
		TargetCompression {
			compressions,
			compression_goal: CompressionGoal::UseBestCompression,
		}
	}

	/// Creates a new `TargetCompression` allowing only the specified compression.
	///
	/// The compression goal is set to `UseBestCompression`.
	///
	/// # Arguments
	///
	/// * `compression` - A single compression algorithm to allow.
	///
	/// # Returns
	///
	/// * `TargetCompression` instance.
	#[must_use]
	pub fn from(compression: TileCompression) -> Self {
		Self::from_set(EnumSet::only(compression))
	}

	/// Creates a new `TargetCompression` allowing no compression.
	///
	/// The compression goal is set to `UseBestCompression`, but since no compression is allowed,
	/// data will remain uncompressed.
	///
	/// # Returns
	///
	/// * `TargetCompression` instance.
	#[must_use]
	pub fn from_none() -> Self {
		Self::from(TileCompression::Uncompressed)
	}

	/// Sets the compression goal to prioritize speed.
	pub fn set_fast_compression(&mut self) {
		self.compression_goal = CompressionGoal::UseFastCompression;
	}

	/// Sets the compression goal to treat data as incompressible.
	pub fn set_incompressible(&mut self) {
		self.compression_goal = CompressionGoal::IsIncompressible;
	}

	/// Checks if a specific compression algorithm is allowed.
	///
	/// # Arguments
	///
	/// * `compression` - The compression algorithm to check.
	///
	/// # Returns
	///
	/// * `true` if the compression is allowed.
	/// * `false` otherwise.
	#[must_use]
	pub fn contains(&self, compression: TileCompression) -> bool {
		self.compressions.contains(compression)
	}

	/// Adds a compression algorithm to the allowed set.
	///
	/// # Arguments
	///
	/// * `compression` - The compression algorithm to add.
	pub fn insert(&mut self, compression: TileCompression) {
		self.compressions.insert(compression);
	}
}

impl Debug for TargetCompression {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("TargetCompression")
			.field("allowed_compressions", &self.compressions)
			.field("compression_goal", &self.compression_goal)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileCompression;
	use enumset::EnumSet;

	#[test]
	fn test_from_set_default_goal() {
		let set = EnumSet::only(TileCompression::Gzip);
		let tc = TargetCompression::from_set(set);
		assert!(tc.contains(TileCompression::Gzip));
		assert_eq!(tc.compression_goal, CompressionGoal::UseBestCompression);
	}

	#[test]
	fn test_from_single_and_contains() {
		let tc = TargetCompression::from(TileCompression::Brotli);
		assert!(tc.contains(TileCompression::Brotli));
		assert!(!tc.contains(TileCompression::Gzip));
		assert_eq!(tc.compression_goal, CompressionGoal::UseBestCompression);
	}

	#[test]
	fn test_from_none() {
		let tc = TargetCompression::from_none();
		assert!(tc.contains(TileCompression::Uncompressed));
		assert_eq!(tc.compressions.len(), 1);
		assert_eq!(tc.compression_goal, CompressionGoal::UseBestCompression);
	}

	#[test]
	fn test_set_fast_and_incompressible() {
		let mut tc = TargetCompression::from_none();
		tc.set_fast_compression();
		assert_eq!(tc.compression_goal, CompressionGoal::UseFastCompression);
		tc.set_incompressible();
		assert_eq!(tc.compression_goal, CompressionGoal::IsIncompressible);
	}

	#[test]
	fn test_insert() {
		let mut tc = TargetCompression::from_none();
		assert!(!tc.contains(TileCompression::Gzip));
		tc.insert(TileCompression::Gzip);
		assert!(tc.contains(TileCompression::Gzip));
	}

	#[test]
	fn test_debug_format() {
		let tc = TargetCompression::from(TileCompression::Gzip);
		let s = format!("{tc:?}");
		assert!(s.starts_with("TargetCompression"));
		assert!(s.contains("Gzip"));
		assert!(s.contains("Use Best Compression"));
	}
}
