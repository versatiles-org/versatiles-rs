//! Vector Tile (MVT) support.
//!
//! This module implements low-level read/write utilities for the Mapbox Vector Tile
//! (MVT) protobuf format. It is organized into several submodules:
//!
//! - [`feature`]: compact per‑feature geometry + tag storage.
//! - [`geometry_type`]: enum for the wire‑level geometry type.
//! - [`layer`]: a single tile layer with key/value tables and features.
//! - [`property_manager`]: manages the global key/value tables used by a layer and
//!   encodes/decodes tag indices.
//! - [`tile`]: the top‑level container that holds multiple layers.
//! - [`value`]: typed MVT property values.
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
