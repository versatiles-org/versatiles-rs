use anyhow::{Result, ensure};
use std::fmt::Debug;

/// Represents allowed sizes of a block of tiles
/// The size is represented as log2(size).
/// For example, TraversalSize { max: 6 } represents block of sizes up to 64 tiles.
#[derive(Clone, PartialEq)]
pub struct TraversalSize {
	min: u8,
	max: u8,
}

impl TraversalSize {
	pub fn new(min: u32, max: u32) -> Result<TraversalSize> {
		assert!(min <= max, "min size must be less than or equal to max size");
		Ok(TraversalSize {
			min: Self::size_to_bits(min)?,
			max: Self::size_to_bits(max)?,
		})
	}

	pub const fn new_default() -> Self {
		TraversalSize { min: 0, max: 31 }
	}

	pub fn new_max(size: u32) -> Result<TraversalSize> {
		TraversalSize::new(0, size)
	}

	pub fn is_empty(&self) -> bool {
		self.min > self.max
	}

	pub fn get_max_size(&self) -> Result<u32> {
		ensure!(!self.is_empty(), "TraversalSize is empty: {self:?}");
		ensure!(self.max <= 32, "TraversalSize max is too large: {self:?}");
		Ok(1 << self.max)
	}

	pub fn intersect(&mut self, other: &TraversalSize) -> Result<()> {
		self.min = self.min.max(other.min);
		self.max = self.max.min(other.max);
		ensure!(
			!self.is_empty(),
			"Non-overlapping traversal sizes: {self:?} and {other:?}"
		);
		Ok(())
	}

	fn size_to_bits(size: u32) -> Result<u8> {
		ensure!(size > 0, "Size must be greater than zero");
		let bits = (size as f64).log2().floor() as u8;
		ensure!(bits < 32, "Size is too large: {size}");
		Ok(bits)
	}
}

impl Default for TraversalSize {
	fn default() -> Self {
		TraversalSize::new_default()
	}
}

impl Debug for TraversalSize {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "TraversalSize [{}..{}]", 1 << self.min, 1 << self.max)
	}
}
