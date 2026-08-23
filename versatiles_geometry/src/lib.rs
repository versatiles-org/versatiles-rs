//! This crate provides geometric data structures and utilities for the VersaTiles ecosystem.
//!
//! It includes modules for:
//! - `geo`: core geometry primitives and traits (e.g., `Point`, `Polygon`, etc.).
//! - `geojson`: parsing and serialization for GeoJSON and NDGeoJSON.
//! - `tile_outline`: helper for generating polygonal outlines from tile bounding boxes.
//! - `vector_tile`: support for reading and writing Mapbox Vector Tile (MVT) protobuf data.
//!
//! These modules form the geometric backbone for reading, transforming, and exporting geospatial data in VersaTiles.
//!
//! # Reading and writing a vector tile
//!
//! ```
//! use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer, layer_stats, validate_tile};
//!
//! // A tile is a list of named layers; encoding produces MVT protobuf bytes.
//! let tile = VectorTile::new(vec![VectorTileLayer::new_standard("places")]);
//! let blob = tile.to_blob()?;
//!
//! // Reading them back gives an equivalent tile.
//! let parsed = VectorTile::from_blob(&blob)?;
//! assert!(parsed.find_layer("places").is_some());
//!
//! // Tiles from elsewhere can be checked against the MVT spec, and measured.
//! assert!(validate_tile(&parsed).is_empty());
//! assert_eq!(layer_stats(&parsed)?.len(), 1);
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod arc_graph;
pub mod ext;
pub mod feature_import;
pub mod feature_source;
pub mod geo;
pub mod geojson;
pub mod tile_outline;
pub mod vector_tile;
