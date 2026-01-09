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
pub use mbtiles::{MBTilesReader, MBTilesWriter};

#[cfg(any(test, feature = "test"))]
mod mock;
#[cfg(any(test, feature = "test"))]
pub use mock::{
	MOCK_BYTES_JPG, MOCK_BYTES_PBF, MOCK_BYTES_PNG, MOCK_BYTES_WEBP, MockReader, MockReaderProfile, MockWriter,
};

mod pmtiles;
pub use pmtiles::{PMTilesReader, PMTilesWriter};

mod tar;
pub use tar::{TarTilesReader, TarTilesWriter};

mod directory;
pub use directory::{DirectoryReader, DirectoryWriter};

mod versatiles;
pub use versatiles::{VersaTilesReader, VersaTilesWriter};
