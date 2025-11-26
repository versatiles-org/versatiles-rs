//! Vector Tile (MVT) support.
//!
//! This module implements low-level read/write utilities for the Mapbox Vector Tile
//! (MVT) protobuf format.
//!
//! Together these pieces allow encoding/decoding full tiles, transforming properties,
//! and converting between vector‑tile features and higher‑level `GeoFeature`s for
//! GeoJSON export.
//!
//! This module re‑exports the most commonly used types for convenience:
//! [`VectorTileLayer`] and [`VectorTile`].

mod feature;
mod geometry_type;
mod layer;
mod property_manager;
mod tile;
mod value;

pub use layer::VectorTileLayer;
pub use tile::VectorTile;
