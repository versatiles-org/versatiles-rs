//! This module provides the `ProgressDummy` struct, a no-op implementation of a progress indicator.
//!
//! # Overview
//!
//! The `ProgressDummy` struct is a no-op implementation of the `ProgressTrait` trait. It provides
//! the same interface as a progress bar but does nothing when its methods are called. This can be useful
//! in situations where a progress indicator is required by an interface, but you do not want any actual
//! progress output.

use super::ProgressTrait;

/// A struct that represents a no-op progress indicator.
pub struct ProgressDummy {}

impl ProgressTrait for ProgressDummy {
	fn new() -> Self {
		Self {}
	}
	fn init(&mut self, _message: &str, _max_value: u64) {}
	fn set_max_value(&mut self, _max_value: u64) {}
	fn set_position(&mut self, _value: u64) {}
	fn inc(&mut self, _value: u64) {}
	fn finish(&mut self) {}
	fn remove(&mut self) {}
}
