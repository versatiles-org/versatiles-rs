//! A zoom level, as a VPL argument writes one.

use std::fmt::{Debug, Display};

use anyhow::{Result, anyhow};

use super::tile_coord::{MAX_ZOOM_LEVEL, validate_zoom_level};

/// A zoom level between `0` and [`MAX_ZOOM_LEVEL`].
///
/// Exists so that a `level_min=` or `max_zoom=` argument carries its range in
/// its type. Seventeen parameters were `u8`, which advertises `0..=255` for
/// something that means `0..=30`: `level_max=99` is a perfectly good `u8` and
/// not a zoom level, so nothing could refuse it until something tried to build
/// a pyramid out of it (#260).
///
/// Thin on purpose. The bound is [`validate_zoom_level`], the same function
/// every coordinate type here uses, rather than a second copy of `<= 30`; and
/// the level comes straight back out as a `u8`, because that is what the tile
/// types take. What this adds is a parser at the edge and a name in the
/// metadata — not a new currency for the internals to convert between.
///
/// # Examples
///
/// ```
/// use versatiles_core::ZoomLevel;
///
/// assert_eq!(u8::from(ZoomLevel::try_from("12").unwrap()), 12);
/// assert!(ZoomLevel::try_from("99").is_err());
/// assert!(ZoomLevel::try_from("half").is_err());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZoomLevel(u8);

impl ZoomLevel {
	/// Builds a zoom level, checking it against [`MAX_ZOOM_LEVEL`].
	///
	/// # Errors
	///
	/// Errors when `level` is deeper than [`MAX_ZOOM_LEVEL`].
	pub fn new(level: u8) -> Result<Self> {
		validate_zoom_level(level)?;
		Ok(Self(level))
	}

	/// The level as the tile types take it.
	#[must_use]
	pub fn level(self) -> u8 {
		self.0
	}
}

impl From<ZoomLevel> for u8 {
	fn from(level: ZoomLevel) -> Self {
		level.0
	}
}

impl TryFrom<&str> for ZoomLevel {
	type Error = anyhow::Error;

	/// Parses the level as VPL writes it.
	///
	/// The two ways to get this wrong are answered separately, because they are
	/// different mistakes: `half` is not a number, and `99` is a number that is
	/// not a zoom level.
	fn try_from(value: &str) -> Result<Self> {
		let text = value.trim();
		let level = text
			.parse::<u8>()
			.map_err(|_| anyhow!("zoom level '{text}' is not a number between 0 and {MAX_ZOOM_LEVEL}"))?;
		Self::new(level)
	}
}

impl Display for ZoomLevel {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		Display::fmt(&self.0, f)
	}
}

impl Debug for ZoomLevel {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "ZoomLevel({})", self.0)
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[test]
	fn the_whole_range_parses() {
		for level in 0..=MAX_ZOOM_LEVEL {
			let parsed = ZoomLevel::try_from(level.to_string().as_str()).unwrap();
			assert_eq!(u8::from(parsed), level);
		}
	}

	/// The gap the type closes: a number a `u8` accepts and a pyramid does not.
	#[rstest]
	#[case::just_past_the_end("31", "must be <= 30")]
	#[case::a_fine_u8("99", "must be <= 30")]
	#[case::past_a_u8("256", "not a number between 0 and 30")]
	#[case::not_a_number("half", "not a number between 0 and 30")]
	#[case::empty("", "not a number between 0 and 30")]
	#[case::negative("-1", "not a number between 0 and 30")]
	fn a_level_outside_the_range_says_which_range(#[case] input: &str, #[case] expected: &str) {
		let error = ZoomLevel::try_from(input).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	#[test]
	fn whitespace_around_a_level_is_accepted() {
		assert_eq!(u8::from(ZoomLevel::try_from(" 7 ").unwrap()), 7);
	}

	/// The bound is the library's, not a second copy of it.
	#[test]
	fn the_deepest_level_is_the_one_the_coordinate_types_allow() {
		assert!(ZoomLevel::new(MAX_ZOOM_LEVEL).is_ok());
		assert!(ZoomLevel::new(MAX_ZOOM_LEVEL + 1).is_err());
	}

	#[test]
	fn levels_order_and_render_as_numbers() {
		assert!(ZoomLevel::new(3).unwrap() < ZoomLevel::new(11).unwrap());
		assert_eq!(ZoomLevel::new(11).unwrap().to_string(), "11");
		assert_eq!(format!("{:?}", ZoomLevel::new(11).unwrap()), "ZoomLevel(11)");
	}
}
