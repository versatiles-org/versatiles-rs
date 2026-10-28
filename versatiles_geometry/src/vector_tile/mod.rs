//! Vector Tile (MVT) support.
//!
//! This module implements low-level read/write utilities for the Mapbox Vector Tile
//! (MVT) protobuf format. With the `mlt` feature, [`VectorTile`] also reads and
//! writes MapLibre Tiles (MLT).
//!
//! Together these pieces allow encoding/decoding full tiles, transforming properties,
//! and converting between vector‑tile features and higher‑level `GeoFeature`s for
//! GeoJSON export.
//!
//! This module re‑exports the most commonly used types for convenience:
//! [`VectorTileLayer`] and [`VectorTile`].
//!
//! ## Arithmetic
//! Geometry and tag ids are decoded from tiles, and geometry is encoded from any
//! input, so integer arithmetic is checked or saturating (#294). The lint keeps
//! new code to that; arithmetic that cannot overflow says why where it is
//! allowed. Tests are exempt.
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

mod feature;
mod geometry_type;
mod layer;
#[cfg(feature = "mlt")]
mod mlt;
mod property_manager;
mod repair;
mod stats;
mod tile;
mod validator;
mod value;

pub use geometry_type::GeomType;
pub use layer::VectorTileLayer;
pub use repair::repair_tile;
pub use stats::{LayerStats, layer_stats, varint_len};
pub use tile::VectorTile;
pub use validator::{DegenerateReason, IssueKind, ValidationIssue, validate_tile};
/// Trait for encoding/decoding individual MVT property values. Exposed so
/// downstream callers (e.g. `TileSizeMonitor`'s breakdown) can measure the
/// encoded byte cost of each value without depending on the full layer
/// encoder.
pub use value::GeoValuePBF;
