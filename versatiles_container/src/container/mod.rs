//! contains various tile container implementations
//!
//! ## Supported tile container formats
//!
//! | Format         | Read | Write |
//! |----------------|:----:|:-----:|
//! | `*.versatiles` | ✅   | ✅     |
//! | `*.mbtiles`    | ✅   | ✅     |
//! | `*.pmtiles`    | ✅   | ✅     |
//! | `*.tar`        | ✅   | ✅     |
//! | directory      | ✅   | ✅     |
//!
//! This module provides a unified interface for reading and writing various tile container formats.
//! Every format is always built; the `sftp` feature adds `sftp://` locations. Pipelines
//! (`.vpl`) are read by `versatiles_pipeline`, not here.

mod mbtiles;
pub use mbtiles::{MBTilesReader, MBTilesTileSink, MBTilesWriter};

#[cfg(any(test, feature = "test"))]
mod mock;
#[cfg(any(test, feature = "test"))]
pub use mock::{
	MOCK_BYTES_JPG, MOCK_BYTES_PBF, MOCK_BYTES_PNG, MOCK_BYTES_WEBP, MockReader, MockReaderProfile, MockWriter,
};

mod pmtiles;
pub use pmtiles::{PMTilesReader, PMTilesWriter};

mod tar;
pub use tar::{TarTileSink, TarTilesReader, TarTilesWriter};

mod directory;
pub use directory::{DirectoryReader, DirectoryTileSink, DirectoryWriter};

mod tile_chunking;
mod tile_path;

mod versatiles;
pub use versatiles::{VersaTilesReader, VersaTilesSink, VersaTilesWriter};
