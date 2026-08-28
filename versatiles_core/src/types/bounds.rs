//! What a numeric parameter accepts, in a form a form can render.

/// The range of numbers a parameter accepts.
///
/// The describing half of a range, to sit beside the checking half.
/// `ValueValidator` is `fn(&[String]) -> Vec<String>` — a predicate. It can say
/// that a value someone already typed is wrong; it cannot tell a form what to
/// offer *before* they type, and the only way to recover a bound from a
/// function is to probe it with candidates and binary-search, which is plainly
/// not the intent. A closed set has `variants()` for this. A range had nothing
/// until now (#260).
///
/// Bounds belong to the *type*, next to `variants()`, for the reason the
/// validator's own documentation gives: a list to keep in step is how the
/// mismatch started. Bounds derived from the type cannot drift from the parser
/// that enforces them; a `#[vpl(range = "0..=30")]` attribute on each field
/// can, and would repeat the number at every site.
///
/// # Why each field is shaped the way it is
///
/// * `min` and `max` are optional, because a byte count has a floor and no
///   useful ceiling, and `None` says that better than `f64::MAX` does.
/// * Inclusivity is explicit at both ends. `raster_levels.gamma` and
///   `.contrast` are documented "above `0`", an exclusive lower bound that a
///   plain min/max pair cannot express — leaving both this project and its
///   consumers to approximate the same two fields.
/// * `integer` tells a consumer whether to step by one. It is recoverable from
///   `rust_type` while a field is a `u8`, and stops being recoverable the
///   moment it becomes a `ZoomLevel` — so typing a field would otherwise take
///   this away.
///
/// `f64` throughout, including for integer ranges: every integer type VPL
/// accepts fits it exactly, and one representation beats a second enum for
/// consumers to switch on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
	/// Smallest accepted value, or `None` where there is no lower end.
	pub min: Option<f64>,
	/// Largest accepted value, or `None` where there is no upper end.
	pub max: Option<f64>,
	/// Whether `min` itself is accepted. Meaningless when `min` is `None`.
	pub min_inclusive: bool,
	/// Whether `max` itself is accepted. Meaningless when `max` is `None`.
	pub max_inclusive: bool,
	/// Whether only whole numbers are accepted.
	pub integer: bool,
}

impl Bounds {
	/// Both ends included, either end optional.
	#[must_use]
	pub const fn new(min: Option<f64>, max: Option<f64>, integer: bool) -> Self {
		Self {
			min,
			max,
			min_inclusive: true,
			max_inclusive: true,
			integer,
		}
	}

	/// Whole numbers from `min` to `max`, both included.
	#[must_use]
	pub const fn integers(min: f64, max: f64) -> Self {
		Self::new(Some(min), Some(max), true)
	}

	/// Any real number, with no end at either side.
	#[must_use]
	pub const fn reals() -> Self {
		Self::new(None, None, false)
	}

	/// Real numbers strictly greater than `min`, with no upper end.
	///
	/// The "above `0`" case, which is a different promise from "`0` or more":
	/// a gamma of zero divides by zero rather than being a mild edge.
	#[must_use]
	pub const fn greater_than(min: f64) -> Self {
		Self {
			min: Some(min),
			max: None,
			min_inclusive: false,
			max_inclusive: true,
			integer: false,
		}
	}
}

/// The range a primitive number type accepts, as a constant on the type itself.
///
/// Lets the `VPLDecode` derive describe a field that has no domain type of its
/// own — `Option<u8>` is `0..=255` whole numbers, and saying so is more useful
/// than saying nothing. The numbers live here rather than in the proc macro,
/// which only names the type.
pub trait NumberBounds {
	/// What this type accepts.
	const BOUNDS: Bounds;
}

macro_rules! integer_bounds {
	($($ty:ty),+ $(,)?) => {
		$(impl NumberBounds for $ty {
			const BOUNDS: Bounds = Bounds::integers(<$ty>::MIN as f64, <$ty>::MAX as f64);
		})+
	};
}

integer_bounds!(u8, u16, u32, i8, i16, i32);

impl NumberBounds for f32 {
	const BOUNDS: Bounds = Bounds::reals();
}

impl NumberBounds for f64 {
	const BOUNDS: Bounds = Bounds::reals();
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_integer_type_describes_its_own_range() {
		assert_eq!(u8::BOUNDS, Bounds::integers(0.0, 255.0));
		assert_eq!(u32::BOUNDS.max, Some(4_294_967_295.0));
		assert_eq!(u16::BOUNDS, Bounds::integers(0.0, 65_535.0));
	}

	#[test]
	fn a_float_type_has_no_ends_but_is_still_worth_describing() {
		// `reals()` is "no end at either side, decimals allowed" — asserting the
		// whole value says both at once, and says which one is wrong on failure.
		assert_eq!(f32::BOUNDS, Bounds::reals());
		assert_eq!(f64::BOUNDS, Bounds::reals());
		assert!(!Bounds::reals().integer, "a form should allow decimals");
	}

	/// The distinction a min/max pair alone cannot make.
	#[test]
	fn an_exclusive_lower_bound_says_so() {
		let above_zero = Bounds::greater_than(0.0);
		assert_eq!(above_zero.min, Some(0.0));
		assert!(!above_zero.min_inclusive, "`0` is not accepted");
		assert_eq!(above_zero.max, None);

		assert!(Bounds::integers(0.0, 30.0).min_inclusive, "`0` is a zoom level");
	}
}
