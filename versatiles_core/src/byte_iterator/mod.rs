//! This module provides utilities for byte-level iteration and related functionality.
//! It re-exports the `basics` and `iterator` modules for use in parsing or reading byte streams.
//!
//! ## Arithmetic
//! Positions count bytes of input that may be arbitrarily long, so arithmetic on
//! them is checked or saturating (#294). The lint keeps new code to that. Tests
//! are exempt.
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

mod basics;
mod iterator;

pub use basics::*;
pub use iterator::*;
