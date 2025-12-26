//! Type definitions for Node.js bindings
//!
//! This module contains all the type definitions and data structures used
//! in the NAPI bindings. These types are exposed to JavaScript and provide
//! configuration options and data structures for tile operations.
//!
//! ## Main Types
//!
//! - [`ConvertOptions`]: Configuration for tile conversion operations
//! - [`ServerOptions`]: Configuration for the HTTP tile server
//! - [`SourceMetadata`]: Metadata about a tile source
//! - [`ProbeResult`]: Information about a probed tile container
//! - [`TileCoord`]: Tile coordinate with zoom, x, and y

mod convert_options;
mod probe_result;
mod server_options;
mod source_metadata;
mod tile_compression;
mod tile_coord;

pub use convert_options::ConvertOptions;
pub use probe_result::ProbeResult;
pub use server_options::ServerOptions;
pub use source_metadata::SourceMetadata;
pub use tile_compression::parse_compression;
pub use tile_coord::TileCoord;
