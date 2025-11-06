//! This crate provides geometric data structures and utilities for the VersaTiles ecosystem.
//!
//! It includes modules for:
//! - `geo`: core geometry primitives and traits (e.g., `Point`, `Polygon`, etc.).
//! - `geojson`: parsing and serialization for GeoJSON and NDGeoJSON.
//! - `tile_outline`: helper for generating polygonal outlines from tile bounding boxes.
//! - `vector_tile`: support for reading and writing Mapbox Vector Tile (MVT) protobuf data.
//!
//! These modules form the geometric backbone for reading, transforming, and exporting geospatial data in VersaTiles.

pub mod geo;
pub mod geojson;
pub mod tile_outline;
pub mod vector_tile;
