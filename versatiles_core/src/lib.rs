//! VersaTiles Core Library
//!
//! This crate provides the core types, utilities, and I/O abstractions for working with
//! map tiles in the VersaTiles ecosystem.
//!
//! # Main Components
//!
//! - **[`types`]**: Core types including tile coordinates ([`TileCoord`]), bounding boxes
//!   ([`TileBBox`], [`GeoBBox`]), formats ([`TileFormat`]), compression ([`TileCompression`]),
//!   and binary data handling ([`Blob`]).
//!
//! - **[`io`]**: I/O abstractions for reading and writing tile data from various sources
//!   (files, HTTP, memory) with support for value serialization in different byte orders.
//!
//! - **[`json`]**: JSON parsing, stringification, and NDJSON (newline-delimited JSON) support
//!   with custom types (`JsonValue`, `JsonArray`, `JsonObject`).
//!
//! - **[`utils`]**: Utility modules for compression, CSV parsing, spatial indexing (Hilbert curves),
//!   and pretty-printing.
//!
//! - **[`concurrency`]**: Concurrency limit tuning for optimal I/O and CPU performance
//!   ([`ConcurrencyLimits`]).
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{TileCoord, TileBBox, Blob, TileFormat};
//!
//! // Create a tile coordinate
//! let coord = TileCoord::new(5, 10, 15).unwrap();
//! println!("Tile: {:?}", coord);
//!
//! // Create a bounding box
//! let bbox = TileBBox::new_full(5).unwrap();
//! println!("BBox covers {} tiles", bbox.count_tiles());
//!
//! // Work with binary data
//! let data = Blob::from("Hello, tiles!");
//! assert_eq!(data.len(), 13);
//! ```

pub mod byte_iterator;
pub mod concurrency;
pub use concurrency::*;
pub mod io;
pub mod json;
pub mod macros;
pub mod types;
pub use types::*;
pub mod utils;
