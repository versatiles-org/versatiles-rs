//! Testing and assertion macros
//!
//! This module provides utility macros for testing, particularly for pattern-based assertions.

/// Asserts that the string representation of an expression matches a given wildcard pattern.
///
/// This macro is useful when you want to verify that an expression's output conforms to
/// a specific wildcard pattern, rather than an exact string match.
///
/// # Example
/// ```
/// use versatiles_core::assert_wildcard;
/// let value = "hello_world";
/// assert_wildcard!(value, "hello_*");
/// ```
#[macro_export]
macro_rules! assert_wildcard {
	($expression:expr, $wildcard:expr) => {
		let expression = format!("{}", $expression);
		if !wildmatch::WildMatch::new($wildcard).matches(&expression) {
			panic!(
				"assertion failed: expression \"{expression:?}\" does not match wildcard \"{}\"",
				$wildcard
			)
		}
	};
}
