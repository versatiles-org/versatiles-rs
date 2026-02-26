//! Core types for working with map tiles
//!
//! This module provides fundamental types for tile-based mapping systems, including:
//!
//! # Spatial Types
//! - [`TileCoord`]: 3D tile coordinates (x, y, zoom level)
//! - [`TileBBox`]: Tile-space bounding boxes for defining rectangular regions
//! - [`GeoBBox`]: Geographic bounding boxes in WGS84 coordinates
//! - [`GeoCenter`]: Geographic center points with zoom level
//!
//! # Tile Metadata
//! - [`TileFormat`]: Tile data formats (PNG, JPG, WebP, MVT, etc.)
//! - [`TileCompression`]: Compression algorithms (Gzip, Brotli, uncompressed)
//! - [`TileType`]: Tile content classification (raster, vector, unknown)
//! - [`TileSchema`]: Tile schema identifiers (RGB, RGBA, OpenMapTiles, etc.)
//! - [`TileSize`]: Pixel dimensions (256×256, 512×512)
//!
//! # Data Handling
//! - [`Blob`]: Binary data wrapper with utility methods
//! - [`ByteRange`]: Byte range specification (offset + length)
//! - [`TileStream`]: Asynchronous tile data streaming
//!
//! # Utilities
//! - [`TileBBoxPyramid`]: Multi-level tile pyramid structure
//! - [`TileBBoxMap`]: Sparse storage for tile bounding boxes
//! - [`LimitedCache`]: Size-limited LRU cache
//! - [`ProbeDepth`]: Tile container inspection depth levels
//!
//! # TileJSON
//! - [`TileJSON`]: TileJSON specification implementation with vector layer metadata support
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{TileCoord, TileBBox, GeoBBox, TileFormat};
//!
//! // Create a tile coordinate at zoom 5
//! let coord = TileCoord::new(5, 10, 15).unwrap();
//!
//! // Create a tile bounding box
//! let tile_bbox = TileBBox::new_full(5).unwrap();
//! assert_eq!(tile_bbox.count_tiles(), 1024); // 32×32 tiles at zoom 5
//!
//! // Convert to geographic bounding box
//! let geo_bbox = tile_bbox.to_geo_bbox().unwrap();
//! println!("Geographic bounds: {:?}", geo_bbox);
//!
//! // Work with tile formats
//! assert_eq!(TileFormat::PNG.as_extension(), ".png");
//! assert_eq!(TileFormat::MVT.is_vector(), true);
//! ```

mod blob;
pub use blob::*;

mod byte_range;
pub use byte_range::*;

mod constants;
pub use constants::*;

mod geo_bbox;
pub use geo_bbox::*;

mod geo_center;
pub use geo_center::*;

mod limited_cache;
pub use limited_cache::*;

mod probe_depth;
pub use probe_depth::*;

mod tile_bbox;
pub use tile_bbox::*;

mod tile_bbox_map;
pub use tile_bbox_map::*;

mod tile_bbox_pyramid;
pub use tile_bbox_pyramid::*;

mod tile_compression;
pub use tile_compression::*;

mod tile_coord;
pub use tile_coord::*;

mod tile_format;
pub use tile_format::*;

mod tilejson;
pub use tilejson::*;

mod tile_schema;
pub use tile_schema::*;

mod tile_size;
pub use tile_size::*;

mod tile_stream;
pub use tile_stream::*;

mod tile_type;
pub use tile_type::*;
