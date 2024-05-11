//! contains various tile container implementations
//!
//! ## Supported tile container formats
//!
//! | Format         | Read | Write | Feature   |
//! |----------------|:----:|:-----:|-----------|
//! | `*.versatiles` | ✅   | ✅     | `default` |
//! | `*.mbtiles`    | ✅   | ⛔️     | `full`    |
//! | `*.pmtiles`    | ✅   | ✅     | `full`    |
//! | `*.tar`        | ✅   | ✅     | `full`    |
//! | directory      | ✅   | ✅     | `default` |
//!

mod types;
pub use types::*;

mod reader;
pub use reader::*;

mod writer;
pub use writer::*;

pub mod directory;

pub mod versatiles;

#[cfg(feature = "full")]
pub mod converter;

#[cfg(feature = "full")]
mod getters;
#[cfg(all(feature = "full", test))]
pub use getters::tests::*;
#[cfg(feature = "full")]
pub use getters::*;

#[cfg(feature = "full")]
mod mbtiles;

#[cfg(feature = "full")]
pub mod pmtiles;

#[cfg(feature = "full")]
pub mod tar;

#[cfg(test)]
pub mod mock;
