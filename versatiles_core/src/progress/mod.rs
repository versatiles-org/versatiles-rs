//! This module provides the main interface for progress indicators, including conditional compilation
//! for different progress implementations.
//!
//! # Overview
//!
//! The module wraps around progress indicator implementations based on the build configuration.
//! By default, it provides a no-op progress drain. If the "full" feature is
//! enabled, it includes a terminal-based progress bar. The `ProgressTrait` trait defines the
//! common interface for all progress indicators, and the `get_progress_bar` function provides
//! a convenient way to create an instance of a progress indicator.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::progress::*;
//!
//! let progress = get_progress_bar("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

mod inner;
mod progress_bar;

pub use progress_bar::{ProgressBar, ProgressData};

/// Factory function to create a progress bar or a no-op progress drain based on the build configuration.
///
/// # Arguments
///
/// * `message` - A message describing the task being performed.
/// * `max_value` - The maximum value of the progress.
///
/// # Returns
///
/// A boxed implementation of `ProgressTrait`.
#[must_use]
pub fn get_progress_bar(message: &str, max_value: u64) -> ProgressBar {
	ProgressBar::new(message, max_value)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_progress_trait_methods() {
		// Create a progress bar and call its methods to ensure no panics
		let progress = get_progress_bar("TestTask", 100);
		// Set to a valid position
		progress.set_position(25);
		// Increment by a value
		progress.inc(10);
		// Finish the progress
		progress.finish();
	}

	#[test]
	fn test_progress_overflow_and_finish() {
		let progress = get_progress_bar("OverflowTest", 5);
		// Set beyond max and inc beyond bounds; should not panic
		progress.set_position(10);
		progress.inc(3);
		progress.finish();
	}
}
