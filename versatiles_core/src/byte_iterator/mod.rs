//! This module provides utilities for byte-level iteration and related functionality.
//! It re-exports the `basics` and `iterator` modules for use in parsing or reading byte streams.

mod basics;
mod iterator;

pub use basics::*;
pub use iterator::*;
