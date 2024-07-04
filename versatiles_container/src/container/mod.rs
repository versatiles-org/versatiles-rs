//! contains various tile container implementations
//!
//! ## Supported tile container formats
//!
//! | Format         | Read | Write | Feature   |
//! |----------------|:----:|:-----:|-----------|
//! | `*.versatiles` | ✅   | ✅     | `default` |
//! | `*.mbtiles`    | ✅   | ✅     | `full`    |
//! | `*.pmtiles`    | ✅   | ✅     | `full`    |
//! | `*.tar`        | ✅   | ✅     | `full`    |
//! | directory      | ✅   | ✅     | `default` |
//! | pipeline       | ✅   | ❌     | `full`    |
//!
//! This module provides a unified interface for reading and writing various tile container formats.
//! Depending on the enabled features, it supports different formats with corresponding read and write capabilities.

mod pipeline;
pub use pipeline::*;

mod converter;
pub use converter::*;

mod getters;
#[cfg(test)]
pub use getters::tests::*;
pub use getters::{get_reader, write_to_filename};

mod mbtiles;
pub use mbtiles::*;

#[cfg(feature = "test")]
mod mock;
#[cfg(feature = "test")]
pub use mock::*;

mod pmtiles;
pub use pmtiles::*;

mod tar;
pub use tar::*;

pub mod tile_converter;

mod directory;
pub use directory::*;

mod versatiles;
pub use versatiles::*;

mod writer;
pub use writer::*;
