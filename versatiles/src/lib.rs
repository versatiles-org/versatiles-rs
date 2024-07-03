//! # VersaTiles
//!
//! VersaTiles is a fast Rust library for reading, writing, and converting between different tile containers.
//!
//! ## Features
//! - **Read and Write**: Supports reading and writing various tile container formats.
//! - **Convert**: Convert between different tile formats and compressions.
//!
//! ## Supported Formats
//! - `*.versatiles`
//! - `*.mbtiles` (requires `full` feature)
//! - `*.pmtiles` (requires `full` feature)
//! - `*.tar` (requires `full` feature)
//! - tiles stored in a local directory
//!
//! ## Usage Example
//!
//! ```rust
//! use versatiles::{
//!     container::{get_reader, write_to_filename},
//!     types::{TileFormat, TilesReader}
//! };
//! use std::path::Path;
//! use anyhow::Result;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Define the input filename (local file or URL)
//!     let input_filename = "../testdata/berlin.pmtiles";
//!     let mut reader = get_reader(input_filename).await?;
//!
//!     // Define the output filename
//!     let output_filename = "../testdata/temp1.versatiles";
//!
//!     // Write the tiles to the output file
//!     write_to_filename(&mut *reader, output_filename).await?;
//!
//!     println!("Tiles have been successfully converted and saved to {output_filename}");
//!     Ok(())
//! }
//! ```

pub use versatiles_container as container;
pub use versatiles_core::*;

#[cfg(feature = "full")]
pub use versatiles_derive as derive;

#[cfg(feature = "full")]
pub use versatiles_geometry as geometry;

#[cfg(feature = "full")]
pub use versatiles_image as image;

#[cfg(feature = "full")]
pub use versatiles_pipeline as pipeline;
