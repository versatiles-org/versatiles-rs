//! This module provides the `ProgressTrait` trait and a factory function `get_progress_bar`
//! for creating progress indicators.
//!
//! # Overview
//!
//! The `ProgressTrait` trait defines the interface for progress indicators. Implementations of this
//! trait can be used to display progress information for long-running tasks. The `get_progress_bar`
//! function provides a convenient way to create an instance of a progress indicator based on the
//! current build configuration.

/// A trait defining the interface for progress indicators.
pub trait ProgressTrait: Send + Sync {
	/// Creates a new instance of the progress indicator.
	///
	/// # Returns
	///
	/// A new instance of the implementing type.
	fn new() -> Self
	where
		Self: Sized;

	/// Initializes the progress indicator.
	///
	/// # Arguments
	///
	/// * `message` - A message describing the task being performed.
	/// * `max_value` - The maximum value of the progress.
	fn init(&mut self, message: &str, max_value: u64);

	/// Sets the maxiumum value of the progress.
	///
	/// # Arguments
	///
	/// * `value` - The new position of the progress.
	fn set_max_value(&mut self, max_value: u64);

	/// Sets the position of the progress.
	///
	/// # Arguments
	///
	/// * `value` - The new position of the progress.
	fn set_position(&mut self, value: u64);

	/// Increases the value of the progress by a given amount.
	///
	/// # Arguments
	///
	/// * `value` - The amount by which to increase the progress.
	fn inc(&mut self, value: u64);

	/// Finishes the progress.
	fn finish(&mut self);

	/// Removes the progress indicator.
	fn remove(&mut self);
}
