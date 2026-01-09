//! `SQLite` file `*.mbtiles` as tile container
//!
//! This module provides structures and implementations for reading and writing tiles to and from an `MBTiles` `SQLite` database.
//!
//! The main components of this module are:
//! - `MBTilesReader`: Reads tiles from an `MBTiles` `SQLite` database.
//! - `MBTilesWriter`: Writes tiles to an `MBTiles` `SQLite` database.

mod reader;
mod writer;

pub use reader::MBTilesReader;
pub use writer::MBTilesWriter;
