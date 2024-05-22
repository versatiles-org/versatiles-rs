//! Tile container mocks for testing
//!
//! This module provides mock implementations of tile readers and writers for testing purposes.
//!
//! ## Submodules
//! - `reader`: Contains mock implementations of tile readers.
//! - `writer`: Contains mock implementations of tile writers.
//!
//! ## Usage
//! These mocks can be used to simulate tile reading and writing operations in tests, allowing you to verify the behavior of your code without relying on actual tile data or I/O operations.

mod reader;
mod writer;

pub use reader::*;
pub use writer::*;
