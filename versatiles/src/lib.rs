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
//!     container::*,
//!     core::*,
//! };
//!
//! #[tokio::main]
//! async fn main() {
//!     // Define the input filename (local file or URL)
//!     let input_filename = "../testdata/berlin.pmtiles";
//!     let config = ProcessingConfig::default();
//!     let registry = versatiles::get_registry(config);
//!     let reader = registry.get_reader(input_filename).await.unwrap();
//!
//!     // Define the output filename
//!     let output_path = std::env::temp_dir().join("temp1.versatiles");
//!
//!     // Write the tiles to the output file
//!     registry.write_to_path(reader, &output_path).await.unwrap();
//!
//!     println!("Tiles have been successfully converted and saved to {output_path:?}");
//! }
//! ```

mod config;
mod helper;
#[cfg(feature = "cli")]
pub mod server;

pub use config::*;
pub use helper::*;
pub use versatiles_container as container;
pub use versatiles_core as core;
pub use versatiles_derive as derive;
pub use versatiles_geometry as geometry;
pub use versatiles_image as image;
pub use versatiles_pipeline as pipeline;
