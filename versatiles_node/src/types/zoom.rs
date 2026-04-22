//! Zoom-level validation shared across NAPI bindings.

use napi::Error as NapiError;
use versatiles_core::MAX_ZOOM_LEVEL;

/// Validate a JavaScript-supplied zoom level (passed as `u32`) and narrow it
/// to `u8` for the core API.
///
/// JS exposes zoom as a 32-bit number; the core uses `u8`. A bare `as u8`
/// silently truncates `z >= 256`, so this helper checks the upper bound
/// (`MAX_ZOOM_LEVEL`, currently 30) before casting.
///
/// # Errors
/// Returns `napi::Error` (with a message naming the offending value and the
/// allowed range) when `z > MAX_ZOOM_LEVEL`.
pub fn z_to_u8(z: u32) -> napi::Result<u8> {
	if z > u32::from(MAX_ZOOM_LEVEL) {
		return Err(NapiError::from_reason(format!(
			"Zoom level {z} is invalid; must be between 0 and {MAX_ZOOM_LEVEL}"
		)));
	}
	#[allow(clippy::cast_possible_truncation)] // bounded by MAX_ZOOM_LEVEL above
	Ok(z as u8)
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[rstest]
	#[case(0u8)]
	#[case(1)]
	#[case(15)]
	#[case(30)]
	fn accepts_in_range(#[case] z: u8) {
		assert_eq!(z_to_u8(u32::from(z)).unwrap(), z);
	}

	#[rstest]
	#[case(31)]
	#[case(255)]
	#[case(256)]
	#[case(u32::MAX)]
	fn rejects_out_of_range(#[case] z: u32) {
		let err = z_to_u8(z).unwrap_err();
		assert!(err.reason.contains(&z.to_string()));
		assert!(err.reason.contains("0 and 30"));
	}
}
