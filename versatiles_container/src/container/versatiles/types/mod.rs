//! Types used for reading and writing `*.versatiles` containers
//!
//! This module defines the core types used for handling `*.versatiles` tile containers. It includes definitions for blocks, file headers, and tile indices, which are essential for reading from and writing to `*.versatiles` files.
//!
//! # Overview
//!
//! The `versatiles` container format is designed to store map tiles in a highly efficient manner, supporting various tile formats and compressions. This module provides the necessary structures and functions to work with these containers.
//!
//! # Types
//!
//! - `BlockDefinition`: Defines a block within the tile container, including its offset, coverage, and byte ranges.
//! - `BlockIndex`: Manages a collection of `BlockDefinition`s, allowing for efficient lookups and conversions.
//! - `FileHeader`: Represents the header of a `versatiles` file, containing metadata about the tile format, compression, and ranges.
//! - `TileIndex`: Manages the byte ranges of individual tiles within the container, allowing for efficient access and modifications.

mod block_definition;
pub use block_definition::BlockDefinition;

mod block_index;
pub use block_index::BlockIndex;

mod file_header;
pub use file_header::FileHeader;

mod tile_index;
pub use tile_index::TileIndex;
