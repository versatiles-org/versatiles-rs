//! This module provides functionality for handling tiles stored in tar archives.
//!
//! It includes implementations for both reading from and writing to tar files that contain tile data.
//!
//! ## Overview
//! The module exposes two primary structs:
//! - `TarTilesReader`: For reading tiles from a tar archive.
//! - `TarTilesWriter`: For writing tiles to a tar archive.
//!
//! ## Usage Example
//!
//! ```no_run
//! use versatiles_container::{TarTilesReader, TarTilesWriter, TilesWriterTrait};
//! use versatiles_core::{TileCoord3, TilesReaderTrait};
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Reading from a tar archive
//!     let tar_path = Path::new("path/to/your/tarfile.tar");
//!     let mut reader = TarTilesReader::open_path(tar_path)?;
//!     let tile_coord = TileCoord3::new(1, 2, 3)?;
//!     let tile_data = reader.get_tile_data(&tile_coord).await?;
//!     if let Some(data) = tile_data {
//!         println!("Tile data: {:?}", data);
//!     }
//!
//!     // Writing to a tar archive
//!     let output_path = Path::new("path/to/output.tar");
//!     let mut writer = TarTilesWriter::write_to_path(&mut reader, output_path).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! The above example demonstrates how to read from an existing tar archive containing tile data
//! and how to write tile data to a new tar archive using `TarTilesReader` and `TarTilesWriter` respectively.

mod reader;
mod writer;

pub use reader::TarTilesReader;
pub use writer::TarTilesWriter;
