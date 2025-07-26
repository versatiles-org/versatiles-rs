use super::compression_goal::CompressionGoal;
use crate::types::TileCompression;
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
