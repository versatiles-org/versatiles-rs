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
//! - [`SourceOptions`]: Configuration for opening a tile source
//! - [`SourceMetadata`]: Metadata about a tile source
//! - [`TileCoord`]: Tile coordinate with zoom, x, and y

mod convert_options;
mod server_options;
mod source_metadata;
mod source_options;
mod tile_compression;
mod tile_coord;
mod tilejson;
mod zoom;

pub use convert_options::ConvertOptions;
pub use server_options::ServerOptions;
pub use source_metadata::SourceMetadata;
pub use source_options::SourceOptions;
pub use tile_compression::parse_compression;
pub use tile_coord::TileCoord;
pub use tilejson::TileJSON;
pub use zoom::z_to_u8;
