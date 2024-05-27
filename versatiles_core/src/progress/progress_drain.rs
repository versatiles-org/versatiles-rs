//! This module provides the `ProgressDrain` struct, a no-op implementation of a progress indicator.
//!
//! # Overview
//!
//! The `ProgressDrain` struct is a no-op implementation of the `ProgressTrait` trait. It provides
//! the same interface as a progress bar but does nothing when its methods are called. This can be useful
//! in situations where a progress indicator is required by an interface, but you do not want any actual
//! progress output.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::progress::{ProgressDrain, ProgressTrait};
//!
//! let mut progress = ProgressDrain::new();
//! progress.init("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

use super::ProgressTrait;

/// A struct that represents a no-op progress indicator.
pub struct ProgressDrain {}

impl ProgressTrait for ProgressDrain {
	/// Creates a new `ProgressDrain` instance.
	fn new() -> Self {
		Self {}
	}

	/// Initializes the progress drain. This method does nothing in this implementation.
	///
	/// # Arguments
	///
	/// * `_message` - A message describing the task being performed.
	/// * `_max_value` - The maximum value of the progress.
	fn init(&mut self, _message: &str, _max_value: u64) {}

	/// Sets the position of the progress. This method does nothing in this implementation.
	///
	/// # Arguments
	///
	/// * `_value` - The new position of the progress.
	fn set_position(&mut self, _value: u64) {}

	/// Increases the value of the progress by a given amount. This method does nothing in this implementation.
	///
	/// # Arguments
	///
	/// * `_value` - The amount by which to increase the progress.
	fn inc(&mut self, _value: u64) {}

	/// Finishes the progress. This method does nothing in this implementation.
	fn finish(&mut self) {}

	/// Removes the progress indicator. This method does nothing in this implementation.
	fn remove(&mut self) {}
}
