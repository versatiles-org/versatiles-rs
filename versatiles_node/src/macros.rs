//! Error conversion utilities
//!
//! Bridges Rust error types to `napi::Error` so failures surface as JavaScript
//! exceptions with their full context chain preserved.
//!
//! Preferred entry point is the [`NapiResultExt`] trait — `result.to_napi()?`
//! works for any `Result<T, E>` where `E: Into<anyhow::Error>` (covers
//! `anyhow::Error`, `serde_json::Error`, `std::io::Error`, `TryFromIntError`,
//! etc.). Add context first via `.context("…")` to enrich the error.
//!
//! The legacy [`napi_result!`] macro is kept for backward compatibility but
//! new code should prefer `.to_napi()`.

use napi::Error as NapiError;

/// Convert `anyhow::Error` to `napi::Error`.
///
/// Formats with `{:#}` so the full context chain is included in the JS error
/// message (e.g. `"Outer context: Middle context: Root cause"`).
pub fn anyhow_to_napi(err: &anyhow::Error) -> NapiError {
	NapiError::from_reason(format!("{err:#}"))
}

/// Convert any error implementing `Into<anyhow::Error>` to `napi::Error`.
///
/// Free-function form. Equivalent to `anyhow_to_napi(&err.into())`. Use this
/// inside a `.map_err(...)` when an extension method isn't ergonomic.
// Allowed until call sites are migrated in subsequent steps.
#[allow(dead_code)]
pub fn to_napi(err: impl Into<anyhow::Error>) -> NapiError {
	anyhow_to_napi(&err.into())
}

/// Extension trait that converts a `Result<T, E>` into `napi::Result<T>` for
/// any error type implementing `Into<anyhow::Error>`.
///
/// # Example
///
/// ```
/// use versatiles_node::macros::NapiResultExt;
/// use anyhow::Context;
///
/// fn parse_and_use(s: &str) -> napi::Result<u32> {
///     // serde_json::Error, std::io::Error, etc. all work directly
///     let parsed: u32 = s.parse().context("not a u32").to_napi()?;
///     Ok(parsed * 2)
/// }
/// ```
// Allowed until call sites are migrated in subsequent steps.
#[allow(dead_code)]
pub trait NapiResultExt<T> {
	/// Convert this `Result` into a `napi::Result`, preserving the full error
	/// chain in the resulting `napi::Error.reason` string.
	fn to_napi(self) -> napi::Result<T>;
}

impl<T, E: Into<anyhow::Error>> NapiResultExt<T> for Result<T, E> {
	fn to_napi(self) -> napi::Result<T> {
		self.map_err(to_napi)
	}
}

/// Convert `Result<T, anyhow::Error>` to `Result<T, napi::Error>`.
///
/// Legacy macro kept for backward compatibility. Prefer
/// [`NapiResultExt::to_napi`] in new code.
#[macro_export]
macro_rules! napi_result {
	($expr:expr) => {
		$expr.map_err(|err| $crate::macros::anyhow_to_napi(&err))
	};
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::{Context, anyhow};
	use rstest::rstest;

	#[test]
	fn test_anyhow_to_napi_conversion() {
		let anyhow_err = anyhow!("Test error message");
		let napi_err = anyhow_to_napi(&anyhow_err);

		assert_eq!(napi_err.reason, "Test error message");
	}

	#[test]
	fn test_anyhow_to_napi_with_context() {
		let anyhow_err = anyhow!("Base error").context("Additional context");
		let napi_err = anyhow_to_napi(&anyhow_err);

		// The error should contain both the context and the base error
		assert!(napi_err.reason.contains("Additional context"));
		assert!(napi_err.reason.contains("Base error"));
	}

	#[test]
	fn test_napi_result_macro_with_ok() {
		let result: anyhow::Result<i32> = Ok(42);
		let converted = napi_result!(result);

		assert!(converted.is_ok());
		assert_eq!(converted.unwrap(), 42);
	}

	#[test]
	fn test_napi_result_macro_with_err() {
		let result: anyhow::Result<i32> = Err(anyhow!("Macro test error"));
		let converted = napi_result!(result);

		assert!(converted.is_err());
		let err = converted.unwrap_err();
		assert_eq!(err.reason, "Macro test error");
	}

	#[test]
	fn test_anyhow_to_napi_with_multiple_contexts() {
		let anyhow_err = anyhow!("Root cause")
			.context("First context")
			.context("Second context")
			.context("Third context");
		let napi_err = anyhow_to_napi(&anyhow_err);

		// All contexts should be preserved in the error message
		assert!(napi_err.reason.contains("Root cause"));
		assert!(napi_err.reason.contains("First context"));
		assert!(napi_err.reason.contains("Second context"));
		assert!(napi_err.reason.contains("Third context"));
	}

	#[rstest]
	#[case("Simple error message")]
	#[case("Error with unicode: 日本語 🦀 Ελληνικά")]
	#[case("Multiline\nerror\nmessage")]
	#[case("Error with\nnewlines\tand\ttabs\nand \"quotes\"")]
	#[case("")]
	fn test_anyhow_to_napi(#[case] message: &str) {
		let anyhow_err = anyhow!(message.to_string());
		let napi_err = anyhow_to_napi(&anyhow_err);
		assert_eq!(napi_err.reason, message);
	}

	#[test]
	fn test_anyhow_to_napi_long_message() {
		let anyhow_err = anyhow!("{}", "a".repeat(1000));
		let napi_err = anyhow_to_napi(&anyhow_err);
		assert_eq!(napi_err.reason.len(), 1000);
		assert_eq!(napi_err.reason, "a".repeat(1000));
	}

	#[test]
	fn test_napi_result_macro_with_string_result() {
		let result: anyhow::Result<String> = Ok("Success".to_string());
		let converted = napi_result!(result);

		assert!(converted.is_ok());
		assert_eq!(converted.unwrap(), "Success");
	}

	#[test]
	fn test_napi_result_macro_with_vec_result() {
		let result: anyhow::Result<Vec<i32>> = Ok(vec![1, 2, 3]);
		let converted = napi_result!(result);

		assert!(converted.is_ok());
		assert_eq!(converted.unwrap(), vec![1, 2, 3]);
	}

	#[test]
	fn test_napi_result_macro_preserves_error_chain() {
		let result: anyhow::Result<()> = Err(anyhow!("Inner error"))
			.context("Middle layer")
			.context("Outer layer");
		let converted = napi_result!(result);

		assert!(converted.is_err());
		let err = converted.unwrap_err();
		assert!(err.reason.contains("Inner error"));
		assert!(err.reason.contains("Middle layer"));
		assert!(err.reason.contains("Outer layer"));
	}

	#[test]
	fn test_napi_result_macro_with_option_unwrap() {
		let result: anyhow::Result<i32> = Some(42).ok_or_else(|| anyhow!("Value was None"));
		let converted = napi_result!(result);

		assert!(converted.is_ok());
		assert_eq!(converted.unwrap(), 42);
	}

	#[test]
	fn test_napi_result_macro_with_option_none() {
		let result: anyhow::Result<i32> = None.ok_or_else(|| anyhow!("Value was None"));
		let converted = napi_result!(result);

		assert!(converted.is_err());
		assert_eq!(converted.unwrap_err().reason, "Value was None");
	}

	// -------------------------------------------------------------------------
	// to_napi() free function and NapiResultExt trait
	// -------------------------------------------------------------------------

	#[test]
	fn test_to_napi_function_with_anyhow_error() {
		let napi_err = to_napi(anyhow!("direct anyhow"));
		assert_eq!(napi_err.reason, "direct anyhow");
	}

	#[test]
	fn test_to_napi_function_with_serde_json_error() {
		// serde_json::Error implements Into<anyhow::Error> via From.
		let parse_err: serde_json::Error = serde_json::from_str::<i32>("not json").unwrap_err();
		let napi_err = to_napi(parse_err);
		assert!(!napi_err.reason.is_empty());
	}

	#[test]
	fn test_to_napi_function_with_io_error() {
		let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
		let napi_err = to_napi(io_err);
		assert!(napi_err.reason.contains("missing"));
	}

	#[test]
	fn test_napi_result_ext_anyhow_ok() {
		let r: anyhow::Result<i32> = Ok(7);
		let converted = r.to_napi();
		assert_eq!(converted.unwrap(), 7);
	}

	#[test]
	fn test_napi_result_ext_anyhow_err_preserves_chain() {
		let r: anyhow::Result<()> = Err(anyhow!("root")).context("middle").context("outer");
		let converted = r.to_napi();
		let err = converted.unwrap_err();
		let reason = &err.reason;
		assert!(reason.contains("outer"));
		assert!(reason.contains("middle"));
		assert!(reason.contains("root"));
	}

	#[test]
	fn test_napi_result_ext_serde_json_err() {
		let r: Result<i32, _> = serde_json::from_str("not json");
		let converted = r.to_napi();
		assert!(converted.is_err());
	}

	#[test]
	fn test_napi_result_ext_with_context_chain() {
		// context("...") then to_napi() — the canonical migration pattern.
		let r: Result<i32, _> = serde_json::from_str("nope");
		let converted = r.context("Invalid pipeline JSON").to_napi();
		let err = converted.unwrap_err();
		let reason = &err.reason;
		assert!(reason.contains("Invalid pipeline JSON"));
	}

	#[test]
	fn test_napi_result_ext_io_err_chain() {
		let r: Result<(), _> = Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"));
		let converted = r.context("opening config").to_napi();
		let err = converted.unwrap_err();
		let reason = &err.reason;
		assert!(reason.contains("opening config"));
		assert!(reason.contains("denied"));
	}
}
