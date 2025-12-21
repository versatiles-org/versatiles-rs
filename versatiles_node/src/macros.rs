//! Error conversion utilities
//!
//! This module provides utilities for converting between Rust's `anyhow::Error`
//! and NAPI's `napi::Error` types, which is essential for bridging Rust error
//! handling with JavaScript/Node.js error handling.
//!
//! The main entry point is the [`napi_result!`] macro, which automatically
//! converts `Result<T, anyhow::Error>` to `Result<T, napi::Error>`.

use napi::Error as NapiError;

/// Convert `anyhow::Error` to `napi::Error`
///
/// This function formats the anyhow error with its full context chain
/// and creates a NAPI error with the formatted message.
///
/// # Arguments
///
/// * `err` - The anyhow error to convert
///
/// # Returns
///
/// A NAPI error containing the formatted error message
pub fn anyhow_to_napi(err: anyhow::Error) -> NapiError {
	NapiError::from_reason(format!("{:#}", err))
}

/// Convert `Result<T, anyhow::Error>` to `Result<T, napi::Error>`
///
/// This macro simplifies error conversion in NAPI bindings by automatically
/// mapping anyhow errors to NAPI errors.
///
/// # Example
///
/// ```
/// use versatiles_node::napi_result;
///
/// fn some_operation() -> anyhow::Result<i32> {
///     Ok(42)
/// }
///
/// fn napi_wrapper() -> napi::Result<i32> {
///     napi_result!(some_operation())
/// }
/// ```
#[macro_export]
macro_rules! napi_result {
	($expr:expr) => {
		$expr.map_err($crate::macros::anyhow_to_napi)
	};
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::anyhow;

	#[test]
	fn test_anyhow_to_napi_conversion() {
		let anyhow_err = anyhow!("Test error message");
		let napi_err = anyhow_to_napi(anyhow_err);

		assert_eq!(napi_err.reason, "Test error message");
	}

	#[test]
	fn test_anyhow_to_napi_with_context() {
		let anyhow_err = anyhow!("Base error").context("Additional context");
		let napi_err = anyhow_to_napi(anyhow_err);

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
}
