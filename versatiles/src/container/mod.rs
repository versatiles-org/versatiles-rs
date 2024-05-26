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
//! | composer       | ✅   | ❌     | `full`    |
//!
//! This module provides a unified interface for reading and writing various tile container formats.
//! Depending on the enabled features, it supports different formats with corresponding read and write capabilities.

#[cfg(feature = "full")]
mod converter;
#[cfg(feature = "full")]
pub use converter::*;

mod directory;
pub use directory::*;

#[cfg(feature = "full")]
mod getters;
#[cfg(all(feature = "full", test))]
pub use getters::tests::*;
#[cfg(feature = "full")]
pub use getters::{get_reader, write_to_filename};

#[cfg(feature = "full")]
mod mbtiles;
#[cfg(feature = "full")]
pub use mbtiles::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
pub use mock::*;

#[cfg(feature = "full")]
mod pmtiles;
#[cfg(feature = "full")]
pub use pmtiles::*;

#[cfg(feature = "full")]
mod tar;
#[cfg(feature = "full")]
pub use tar::*;

mod types;
pub use types::*;

mod reader;
pub use reader::*;

mod versatiles;
pub use versatiles::*;

mod writer;
pub use writer::*;

#[cfg(feature = "full")]
mod composer;
#[cfg(feature = "full")]
pub use composer::*;
