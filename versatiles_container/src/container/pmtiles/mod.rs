//! Provides functionality for reading and writing tile data in a `PMTiles` container format.
//!
//! This module contains the primary components for working with `PMTiles` containers:
//! - `PMTilesReader` for reading tile data.
//! - `PMTilesWriter` for writing tile data.
//!
//! ## Features
//! - Efficient reading and writing of tile data with compression support.
//! - Metadata management for `PMTiles` containers.
//!
//! ## Errors
//! - Returns errors if there are issues with reading, writing, or compressing data, or internal processing.
//!
//! ## Arithmetic
//! Offsets, lengths, tile ids and run lengths come out of the file, so
//! arithmetic on them is checked and fails with an error rather than a panic or
//! a wrapped value (#294). The lint keeps new code to that; arithmetic that
//! cannot overflow says why where it is allowed. Tests are exempt: they read
//! no files they did not build themselves.
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]
//!
//! ## Testing
//! This module includes comprehensive tests to ensure the correct functionality of reading and writing metadata, handling different tile formats, and verifying the integrity of the data.

mod reader;
mod types;
mod writer;

pub use reader::PMTilesReader;
pub use writer::PMTilesWriter;
