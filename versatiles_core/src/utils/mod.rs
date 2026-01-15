//! This module provides general-purpose utility modules for common functionality across the codebase.
//! It includes:
//! - `csv`: for lightweight CSV parsing utilities.
//! - `pretty_print` (enabled with the `cli` feature): for formatted command-line output.
//! - `tile_hilbert_index`: for Hilbert index calculations and spatial ordering of tiles.

mod csv;
#[cfg(feature = "cli")]
mod pretty_print;
mod primitives;
mod tile_hilbert_index;

pub use csv::*;
#[cfg(feature = "cli")]
pub use pretty_print::*;
pub use primitives::*;
pub use tile_hilbert_index::*;
