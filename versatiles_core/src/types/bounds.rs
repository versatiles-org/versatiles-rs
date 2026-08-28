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

/// Declares a number type that carries the range it accepts.
///
/// The bound belongs to a *type* rather than to each field that has it, for the
/// same reason `variants()` does: a fact repeated at every site is free to
/// drift, and four bounds in this codebase are already shared by two parameters
/// each. The type gives one parser with two callers — the accessor the builder
/// uses and the probe `check` runs — so a value `check` accepts and a value
/// that builds are the same set by construction.
///
/// Two spellings, because two of the parameters this exists for are documented
/// "above `0`", which an inclusive pair cannot express. The range lands on the
/// type as `MIN`, `MAX` and `MIN_INCLUSIVE`.
///
/// Works for any number `f64` can hold exactly — `u8` through `u32`, `i8`
/// through `i32`, and `f32`.
///
/// ```
/// use versatiles_core::bounded_number;
///
/// bounded_number! {
///     /// Encoder effort, `0` (fastest) to `100` (smallest).
///     Effort: u8, 0..=100;
///     /// Gamma exponent.
///     Gamma: f32, greater_than 0.0;
/// }
///
/// assert_eq!(Effort::try_from("80").unwrap().get(), 80);
/// assert_eq!(Effort::MAX, Some(100));
/// assert!(Effort::try_from("200").is_err());
///
/// assert!(Gamma::try_from("0").is_err(), "`0` is not above `0`");
/// assert_eq!(Gamma::MAX, None, "no upper end");
/// assert!(!Gamma::bounds().min_inclusive);
/// ```
#[macro_export]
macro_rules! bounded_number {
	() => {};

	// An inclusive range: `-255.0..=255.0`.
	(
		$(#[$meta:meta])*
		$name:ident : $inner:ty, $min:literal ..= $max:literal ;
		$($rest:tt)*
	) => {
		$crate::bounded_number!(@define $(#[$meta])* $name : $inner,
			min = $min, max = ::core::option::Option::Some($max), min_inclusive = true,
			reason = ::core::concat!("must be between ", ::core::stringify!($min), " and ", ::core::stringify!($max)));
		$crate::bounded_number!($($rest)*);
	};

	// An exclusive lower bound with no upper end: `greater_than 0.0`.
	(
		$(#[$meta:meta])*
		$name:ident : $inner:ty, greater_than $min:literal ;
		$($rest:tt)*
	) => {
		$crate::bounded_number!(@define $(#[$meta])* $name : $inner,
			min = $min, max = ::core::option::Option::None, min_inclusive = false,
			reason = ::core::concat!("must be greater than ", ::core::stringify!($min)));
		$crate::bounded_number!($($rest)*);
	};

	(
		@define
		$(#[$meta:meta])*
		$name:ident : $inner:ty,
		min = $min:literal, max = $max:expr, min_inclusive = $min_inclusive:literal,
		reason = $reason:expr
	) => {
		$(#[$meta])*
		///
		/// Declared with [`bounded_number!`](crate::bounded_number), so the range
		/// it accepts and the range it advertises are one fact.
		#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
		pub struct $name($inner);

		impl $name {
			/// The smallest value this type accepts, or the value it must
			/// exceed when [`MIN_INCLUSIVE`](Self::MIN_INCLUSIVE) is false.
			pub const MIN: $inner = $min;
			/// The largest value this type accepts, or `None` where there is no
			/// upper end.
			pub const MAX: ::core::option::Option<$inner> = $max;
			/// Whether [`MIN`](Self::MIN) itself is accepted.
			pub const MIN_INCLUSIVE: bool = $min_inclusive;

			/// Builds the value, checking it against the range.
			///
			/// # Errors
			///
			/// Errors outside the range this type declares.
			pub fn new(value: $inner) -> ::anyhow::Result<Self> {
				let above_floor = if Self::MIN_INCLUSIVE { value >= Self::MIN } else { value > Self::MIN };
				let below_ceiling = match Self::MAX {
					::core::option::Option::Some(max) => value <= max,
					::core::option::Option::None => true,
				};
				::anyhow::ensure!(
					above_floor && below_ceiling,
					::core::concat!(::core::stringify!($name), " ", $reason, ", but is {}"),
					value
				);
				::core::result::Result::Ok(Self(value))
			}

			/// The value itself.
			#[must_use]
			pub fn get(self) -> $inner {
				self.0
			}

			/// What a form should offer: the range this type accepts.
			#[must_use]
			pub fn bounds() -> $crate::Bounds {
				$crate::Bounds {
					min: ::core::option::Option::Some(f64::from(Self::MIN)),
					max: match Self::MAX {
						::core::option::Option::Some(max) => ::core::option::Option::Some(f64::from(max)),
						::core::option::Option::None => ::core::option::Option::None,
					},
					min_inclusive: Self::MIN_INCLUSIVE,
					max_inclusive: true,
					integer: <$inner as $crate::NumberBounds>::BOUNDS.integer,
				}
			}
		}

		impl ::core::convert::From<$name> for $inner {
			fn from(value: $name) -> Self {
				value.0
			}
		}

		impl ::core::convert::TryFrom<&str> for $name {
			type Error = ::anyhow::Error;

			fn try_from(value: &str) -> ::anyhow::Result<Self> {
				let text = value.trim();
				let parsed = text.parse::<$inner>().map_err(|_| {
					::anyhow::anyhow!(::core::concat!(::core::stringify!($name), " '{}' is not a number"), text)
				})?;
				Self::new(parsed)
			}
		}

		impl ::core::fmt::Display for $name {
			fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
				::core::fmt::Display::fmt(&self.0, f)
			}
		}
	};
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
