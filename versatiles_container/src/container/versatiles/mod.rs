//! `*.versatiles` container
//!
//! see [specification](https://github.com/versatiles-org/versatiles-spec)
//!
//! This module provides functionality to read from and write to `*.versatiles` container files.
//!
//! # Usage Example
//!
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::{path::Path, sync::Arc};
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let path_mbtiles = std::env::current_dir()?.join("../testdata/berlin.mbtiles");
//!     let path_versatiles = std::env::temp_dir().join("temp4.versatiles");
//!
//!     let runtime = TilesRuntime::default();
//!
//!     // Open the source and convert to .versatiles (conversion computes the tile pyramid)
//!     let reader = runtime.reader_from_str(path_mbtiles.to_str().unwrap()).await?;
//!     convert_tiles_container(reader, TilesConverterParameters::default(), &path_versatiles, runtime.clone()).await?;
//!
//!     println!("Tiles have been successfully written to {path_versatiles:?}");
//!
//!     // Read the tiles back from the .versatiles file
//!     let reader = VersaTilesReader::open(&path_versatiles, runtime).await?;
//!
//!     // Get tile data
//!     if let Some(tile) = reader.tile(&TileCoord::new(12, 2200, 1345)?).await? {
//!         println!("Tile data: {tile:?}");
//!     } else {
//!         println!("No tile data found");
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Arithmetic
//! Offsets, lengths and block coordinates come out of the file, so arithmetic
//! on them is checked and fails with an error rather than a panic or a wrapped
//! value (#294). The lint keeps new code to that; arithmetic that cannot
//! overflow says why where it is allowed. Tests are exempt: they read no files
//! they did not build themselves.
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

mod types;

mod reader;
pub use reader::VersaTilesReader;

mod sink;
pub use sink::VersaTilesSink;

mod writer;
pub use writer::VersaTilesWriter;
