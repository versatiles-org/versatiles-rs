//! `SQLite` file `*.mbtiles` as tile container
//!
//! This module provides structures and implementations for reading and writing tiles to and from an `MBTiles` `SQLite` database.
//!
//! The main components of this module are:
//! - `MBTilesReader`: Reads tiles from an `MBTiles` `SQLite` database.
//! - `MBTilesWriter`: Writes tiles to an `MBTiles` `SQLite` database.

mod reader;
mod sink;
mod writer;

use anyhow::{Result, bail};
pub use reader::MBTilesReader;
pub use sink::MBTilesTileSink;
use versatiles_core::{TileCompression, TileFormat};
pub use writer::MBTilesWriter;

/// How MBTiles stores a given tile format: the value for its `format` metadata
/// row, and the one compression it accepts.
///
/// MBTiles allows exactly one compression per format — raster formats
/// uncompressed, vector tiles gzipped — so this is a mapping rather than a
/// check. Shared by the writer, which recompresses to match, and the sink,
/// which only receives finished blobs and can therefore just verify.
pub(crate) fn required_encoding(format: TileFormat) -> Result<(&'static str, TileCompression)> {
	use TileCompression::{Gzip, Uncompressed};
	use TileFormat::{JPG, MVT, PNG, WEBP};

	Ok(match format {
		JPG => ("jpg", Uncompressed),
		MVT => ("pbf", Gzip),
		PNG => ("png", Uncompressed),
		WEBP => ("webp", Uncompressed),
		other => bail!("MBTiles cannot store {other} tiles; it supports jpg, png, webp and pbf (mvt)"),
	})
}
