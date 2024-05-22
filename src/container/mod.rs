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
//!
//! This module provides a unified interface for reading and writing various tile container formats.
//! Depending on the enabled features, it supports different formats with corresponding read and write capabilities.

mod types;
pub use types::*;

mod reader;
pub use reader::*;

mod writer;
pub use writer::*;

mod directory;
pub use directory::*;

mod versatiles;
pub use versatiles::*;

#[cfg(feature = "full")]
mod converter;
#[cfg(feature = "full")]
pub use converter::*;

#[cfg(feature = "full")]
mod getters;
#[cfg(all(feature = "full", test))]
pub use getters::tests::*;
#[cfg(feature = "full")]
pub use getters::*;

#[cfg(feature = "full")]
mod mbtiles;
#[cfg(feature = "full")]
pub use mbtiles::*;

#[cfg(feature = "full")]
mod pmtiles;
#[cfg(feature = "full")]
pub use pmtiles::*;

#[cfg(feature = "full")]
mod tar;
#[cfg(feature = "full")]
pub use tar::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
pub use mock::*;
