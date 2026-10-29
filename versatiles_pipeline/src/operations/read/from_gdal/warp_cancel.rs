//! Stopping a GDAL warp part-way when its request is cancelled (#265).
//!
//! The warp is the expensive part of a GDAL tile, and it runs on the blocking
//! pool, where dropping the request's future does not reach it. GDAL asks a
//! progress callback whether to go on as it works through the warp; this one
//! answers with the request's [`CancelToken`].

use std::ffi::{c_char, c_int, c_void};

use gdal_sys::GDALWarpOptions;
use versatiles_core::CancelToken;

/// Makes the warp configured by `options` ask `cancel` whether to go on.
///
/// # Safety
/// `options` must point to valid warp options, and `cancel` must outlive every
/// warp run with them: GDAL keeps a pointer to it.
pub(super) unsafe fn stop_warp_when_cancelled(options: *mut GDALWarpOptions, cancel: &CancelToken) {
	unsafe {
		(*options).pfnProgress = Some(keep_going);
		(*options).pProgressArg = std::ptr::from_ref(cancel).cast_mut().cast::<c_void>();
	}
}

/// GDAL's progress callback: nonzero to go on, zero to stop.
///
/// May be called from GDAL's worker threads, which `CancelToken` allows.
unsafe extern "C" fn keep_going(_complete: f64, _message: *const c_char, arg: *mut c_void) -> c_int {
	// SAFETY: `arg` is the `CancelToken` set in `stop_warp_when_cancelled`,
	// which outlives the warp.
	let cancel = unsafe { &*arg.cast_const().cast::<CancelToken>() };
	c_int::from(!cancel.is_cancelled())
}

#[cfg(test)]
mod tests {
	use std::ptr;

	use super::*;

	fn answer(cancel: &CancelToken) -> c_int {
		let arg = ptr::from_ref(cancel).cast_mut().cast::<c_void>();
		unsafe { keep_going(0.5, ptr::null(), arg) }
	}

	#[test]
	fn the_callback_stops_the_warp_once_cancelled() {
		let cancel = CancelToken::new();
		assert_eq!(answer(&cancel), 1);
		cancel.cancel();
		assert_eq!(answer(&cancel), 0);
		assert_eq!(answer(&CancelToken::never()), 1);
	}
}
