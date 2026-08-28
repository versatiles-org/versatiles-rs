//! Defines the `TileSize` enum representing supported raster or vector tile sizes.

use std::fmt::Debug;

use anyhow::{Result, anyhow, bail};

use crate::utils::float_to_int;

/// Represents the pixel dimensions of a map tile.
/// Currently supports 256×256 and 512×512 tiles.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TileSize {
	/// Represents a tile size of 256×256 pixels.
	Size256,
	/// Represents a tile size of 512×512 pixels.
	Size512,
}

impl TileSize {
	/// Constructs a `TileSize` from a `u16` value.
	///
	/// # Arguments
	///
	/// * `size` - The pixel dimension (256 or 512)
	///
	/// # Errors
	///
	/// Returns an error if the size is not 256 or 512.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileSize;
	///
	/// let size = TileSize::new(256).unwrap();
	/// assert_eq!(size.size(), 256);
	///
	/// assert!(TileSize::new(128).is_err());
	/// ```
	pub fn new(size: u16) -> Result<Self> {
		match size {
			256 => Ok(Self::Size256),
			512 => Ok(Self::Size512),
			_ => bail!("Invalid tile size: {size}. Supported sizes are 256 or 512."),
		}
	}

	/// The sizes a picker should offer, written as VPL writes them.
	///
	/// This is what makes `tile_size` a closed set downstream: the TypeScript
	/// bindings render it as `256 | 512` and an editor can offer both, instead
	/// of every consumer rediscovering the range from a doc comment (#260).
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileSize;
	///
	/// assert_eq!(TileSize::variants(), ["256", "512"]);
	/// ```
	#[must_use]
	pub fn variants() -> &'static [&'static str] {
		&["256", "512"]
	}

	/// Returns the size of the tile in pixels.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileSize;
	///
	/// assert_eq!(TileSize::Size256.size(), 256);
	/// assert_eq!(TileSize::Size512.size(), 512);
	/// ```
	#[must_use]
	pub fn size(&self) -> u16 {
		match self {
			TileSize::Size256 => 256,
			TileSize::Size512 => 512,
		}
	}
}

impl Debug for TileSize {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "TileSize({})", self.size())
	}
}

impl TryFrom<f64> for TileSize {
	type Error = anyhow::Error;

	fn try_from(value: f64) -> Result<Self> {
		TileSize::new(float_to_int(value)?)
	}
}

impl TryFrom<&str> for TileSize {
	type Error = anyhow::Error;

	/// Parses the size as VPL writes it, so a `tile_size=` argument is judged
	/// by this type rather than by whatever happens to read it later.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileSize;
	///
	/// assert_eq!(TileSize::try_from("512").unwrap().size(), 512);
	/// assert!(TileSize::try_from("1024").is_err());
	/// assert!(TileSize::try_from("big").is_err());
	/// ```
	fn try_from(value: &str) -> Result<Self> {
		let text = value.trim();
		let size = text
			.parse::<u16>()
			.map_err(|_| anyhow!("Invalid tile size: '{text}'. Supported sizes are 256 or 512."))?;
		Self::new(size)
	}
}

impl TryFrom<u16> for TileSize {
	type Error = anyhow::Error;
	fn try_from(value: u16) -> Result<Self> {
		TileSize::new(value)
	}
}

impl TryFrom<u32> for TileSize {
	type Error = anyhow::Error;
	fn try_from(value: u32) -> Result<Self> {
		TileSize::new(value.try_into()?)
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case(256, TileSize::Size256)]
	#[case(512, TileSize::Size512)]
	fn new_accepts_supported_sizes(#[case] size: u16, #[case] expected: TileSize) {
		let ts = TileSize::new(size).expect("expected Ok for supported size");
		assert_eq!(ts, expected);
		assert_eq!(ts.size(), size);
		assert_eq!(format!("{ts:?}"), format!("TileSize({:?})", size));
	}

	#[rstest]
	#[case(0)]
	#[case(1)]
	#[case(255)]
	#[case(257)]
	#[case(511)]
	#[case(513)]
	fn new_rejects_unsupported_sizes(#[case] input: u16) {
		let err = TileSize::new(input).expect_err("expected Err for unsupported size");
		let msg = format!("{err}");
		assert!(msg.contains("Invalid tile size"));
	}

	/// Every advertised variant parses, and parsing is the only way in — so the
	/// list a picker offers cannot drift from the set the parser accepts.
	#[test]
	fn every_advertised_variant_parses() {
		for &variant in TileSize::variants() {
			let parsed = TileSize::try_from(variant).unwrap_or_else(|_| panic!("`{variant}` is advertised but rejected"));
			assert_eq!(parsed.size().to_string(), variant);
		}
		assert_eq!(
			TileSize::variants().len(),
			2,
			"a new size needs a look at the TS bindings too"
		);
	}

	#[rstest]
	#[case::unsupported("1024", "Supported sizes are 256 or 512")]
	#[case::not_a_number("big", "Invalid tile size: 'big'")]
	#[case::empty("", "Invalid tile size: ''")]
	fn parsing_a_bad_size_says_what_is_accepted(#[case] input: &str, #[case] expected: &str) {
		let error = TileSize::try_from(input).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	#[test]
	fn whitespace_around_a_size_is_accepted() {
		assert_eq!(TileSize::try_from(" 256 ").unwrap(), TileSize::Size256);
	}

	#[test]
	fn clone_copy_and_eq_work() {
		let a = TileSize::Size256;
		let b = a; // Copy
		#[expect(
			clippy::clone_on_copy,
			reason = "the assertion below is that `Clone` also works on this `Copy` type"
		)]
		let c = a.clone(); // Clone
		assert_eq!(a, b);
		assert_eq!(b, c);
	}

	#[rstest]
	#[case::f64_256(256.0_f64, Some(TileSize::Size256))]
	#[case::f64_512(512.0_f64, Some(TileSize::Size512))]
	#[case::f64_invalid(128.0_f64, None)]
	fn try_from_f64(#[case] input: f64, #[case] expected: Option<TileSize>) {
		let result = TileSize::try_from(input);
		match expected {
			Some(e) => assert_eq!(result.unwrap(), e),
			None => assert!(result.is_err()),
		}
	}

	#[rstest]
	#[case::u16_256(256_u16, Some(TileSize::Size256))]
	#[case::u16_512(512_u16, Some(TileSize::Size512))]
	#[case::u16_invalid(128_u16, None)]
	fn try_from_u16(#[case] input: u16, #[case] expected: Option<TileSize>) {
		let result = TileSize::try_from(input);
		match expected {
			Some(e) => assert_eq!(result.unwrap(), e),
			None => assert!(result.is_err()),
		}
	}

	#[rstest]
	#[case::u32_256(256_u32, Some(TileSize::Size256))]
	#[case::u32_512(512_u32, Some(TileSize::Size512))]
	#[case::u32_too_large(1_000_000_u32, None)]
	#[case::u32_invalid(128_u32, None)]
	fn try_from_u32(#[case] input: u32, #[case] expected: Option<TileSize>) {
		let result = TileSize::try_from(input);
		match expected {
			Some(e) => assert_eq!(result.unwrap(), e),
			None => assert!(result.is_err()),
		}
	}
}
