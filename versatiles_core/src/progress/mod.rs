//! This module provides the main interface for progress indicators, including conditional compilation
//! for different progress implementations.
//!
//! # Overview
//!
//! The module conditionally includes different progress indicator implementations based on the
//! build configuration. By default, it provides a no-op progress drain. If the "full" feature is
//! enabled, it includes a terminal-based progress bar. The `ProgressTrait` trait defines the
//! common interface for all progress indicators, and the `get_progress_bar` function provides
//! a convenient way to create an instance of a progress indicator.
//!
//! # Examples
//!
//! ```rust
//! use versatiles::progress::*;
//!
//! let mut progress = get_progress_bar("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

#![allow(unused)]

#[cfg(all(not(feature = "test"), feature = "cli"))]
mod progress_bar;

#[cfg(any(feature = "test", not(feature = "cli")))]
mod progress_dummy;

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
pub fn get_progress_bar(message: &str, max_value: u64) -> Box<dyn ProgressTrait> {
	#[cfg(all(not(feature = "test"), feature = "cli"))]
	let mut progress = progress_bar::ProgressBar::new();
	#[cfg(any(feature = "test", not(feature = "cli")))]
	let mut progress = progress_dummy::ProgressDummy::new();
	progress.init(message, max_value);
	Box::new(progress)
}

mod traits;
pub use traits::ProgressTrait;
