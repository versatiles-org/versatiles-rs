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

mod mbtiles;
pub use mbtiles::*;

#[cfg(any(test, feature = "test"))]
mod mock;
#[cfg(any(test, feature = "test"))]
pub use mock::*;

mod pmtiles;
pub use pmtiles::*;

mod tar;
pub use tar::*;

mod directory;
pub use directory::*;

mod versatiles;
pub use versatiles::*;
