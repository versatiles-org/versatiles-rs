//! Defines the [`CacheKey`] trait used by the caching subsystem.
//!
//! A `CacheKey` represents a serializable and human-readable identifier used to
//! store and retrieve cached items. The key must be unique and stable so that
//! the same input always produces the same cache entry.
//!
//! This trait is implemented for common primitive and domain types such as
//! `String`, `&str`, `usize`, and [`TileCoord`](versatiles_core::TileCoord).

use std::fmt::Debug;
use versatiles_core::TileCoord;

/// Trait defining how an object can be converted into a unique cache key string.
///
/// Implementations should ensure the returned string is deterministic and collision-free
/// within the context of the cache.
///
/// The string is used directly as part of filenames or map keys for both
/// in-memory and on-disk caches.
pub trait CacheKey: Debug {
	/// Convert the object into a unique, human-readable cache key string.
	fn to_cache_key(&self) -> String;
}

/// Uses the string itself as the cache key.
impl CacheKey for String {
	fn to_cache_key(&self) -> String {
		self.clone()
	}
}

/// Converts the string slice into an owned `String` to use as the cache key.
impl CacheKey for &str {
	fn to_cache_key(&self) -> String {
		(*self).to_string()
	}
}

/// Converts the number into its decimal string representation.
impl CacheKey for usize {
	fn to_cache_key(&self) -> String {
		self.to_string()
	}
}

/// Converts the [`TileCoord`](versatiles_core::TileCoord) into a structured key string.
///
/// The format is `"ZZ,XXX,YYY"` with zero-padded fields depending on zoom level (`ZZ`).
/// Padding increases with higher zoom levels to preserve lexicographic ordering.
impl CacheKey for TileCoord {
	fn to_cache_key(&self) -> String {
		let z = self.level;
		let x = self.x;
		let y = self.y;
		match self.level {
			0..=3 => format!("{z:0>2},{x},{y}"),
			4..=6 => format!("{z:0>2},{x:0>2},{y:0>2}"),
			7..=9 => format!("{z:0>2},{x:0>3},{y:0>3}"),
			10..=13 => format!("{z:0>2},{x:0>4},{y:0>4}"),
			14..=16 => format!("{z:0>2},{x:0>5},{y:0>5}"),
			17..=19 => format!("{z:0>2},{x:0>6},{y:0>6}"),
			20..=23 => format!("{z:0>2},{x:0>7},{y:0>7}"),
			24..=26 => format!("{z:0>2},{x:0>8},{y:0>8}"),
			27..=29 => format!("{z:0>2},{x:0>9},{y:0>9}"),
			_ => format!("{z:0>2},{x:0>10},{y:0>10}"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	fn roundtrip<K: CacheKey>(k: K, hash: &str) {
		assert_eq!(k.to_cache_key(), hash);
		assert_eq!(k.to_cache_key(), hash);
	}

	#[test]
	fn works_with_string() {
		roundtrip(String::from("a=b|c=d"), "a=b|c=d");
		roundtrip(String::from("tile:z=5/x=10/y=12"), "tile:z=5/x=10/y=12");
	}

	#[test]
	fn works_with_str() {
		roundtrip("a=b|c=d", "a=b|c=d");
		roundtrip("tile:z=5/x=10/y=12", "tile:z=5/x=10/y=12");
	}

	#[rstest]
	#[case(0usize, "0")]
	#[case(1usize, "1")]
	#[case(42usize, "42")]
	#[case(999_999usize, "999999")]
	fn works_with_usize(#[case] n: usize, #[case] expected: &str) {
		assert_eq!(n.to_cache_key(), expected);
	}

	#[rstest]
	#[case(3, 1, 12, "03,1,12")]
	#[case(4, 1, 2, "04,01,02")]
	#[case(7, 5, 23, "07,005,023")]
	#[case(10, 7, 8, "10,0007,0008")]
	#[case(14, 9, 11, "14,00009,00011")]
	#[case(17, 9, 11, "17,000009,000011")]
	#[case(20, 9, 11, "20,0000009,0000011")]
	#[case(24, 9, 11, "24,00000009,00000011")]
	#[case(27, 9, 11, "27,000000009,000000011")]
	#[case(30, 9, 11, "30,0000000009,0000000011")]
	fn works_with_tilecoord(#[case] level: u8, #[case] x: u32, #[case] y: u32, #[case] expected: &str) {
		assert_eq!(TileCoord { x, y, level }.to_cache_key(), expected);
	}
}
