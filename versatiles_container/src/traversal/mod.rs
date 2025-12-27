//! This module provides traversal utilities and logic for handling data structures
//! in various orders and sizes. It re-exports the `main`, `order`, `processing`,
//! and `size` submodules, which collectively provide traversal control, ordering logic,
//! processing strategies, and size calculations.

mod main;
mod order;
mod processing;
mod size;

pub use main::*;
pub use order::*;
pub use processing::*;
pub use size::*;
