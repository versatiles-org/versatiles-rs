//! # VersaTiles
//!
//! VersaTiles is a fast Rust library for reading, writing, and converting between different tile containers.
//!
//! ## Features
//! - **Read and Write**: Supports reading and writing various tile container formats.
//! - **Convert**: Convert between different tile formats and compressions.
//!
//! ## Feature Flags
//!
//! - **cli** *(default)*: Enables command-line interface support with colored output and progress indicators.
//! - **server** *(default)*: Enables HTTP tile server functionality using axum.
//! - **gdal**: Enables GDAL raster data source support for reading geospatial formats like GeoTIFF.
//!   Requires GDAL to be installed on the system.
//! - **bindgen**: Compiles GDAL bindings from source instead of using pre-built bindings.
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
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let runtime = TilesRuntime::default();
//!     let reader = runtime.get_reader_from_str("../testdata/berlin.pmtiles").await.unwrap();
//!
//!     // Define the output filename
//!     let output_path = std::env::temp_dir().join("temp1.versatiles");
//!
//!     // Write the tiles to the output file
//!     runtime.write_to_path(reader, &output_path).await.unwrap();
//!
//!     println!("Tiles have been successfully converted and saved to {output_path:?}");
//! }
//! ```

pub mod config;
pub mod runtime;
#[cfg(feature = "server")]
pub mod server;

pub use versatiles_container as container;
pub use versatiles_core as core;
pub use versatiles_derive as derive;
pub use versatiles_geometry as geometry;
pub use versatiles_image as image;
pub use versatiles_pipeline as pipeline;
