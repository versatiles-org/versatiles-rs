//! The main `geo` module of the `versatiles_geometry` crate.
//!
//! This module provides data structures and traits for representing GeoJSON-like geometries and features,
//! including geometric primitives (`Point`, `LineString`, `Polygon`, and their multi-geometry counterparts).
//! It also includes `GeoFeature` and `GeoCollection` for representing features and collections of features,
//! as well as `GeoProperties` and `GeoValue` for typed attribute storage.
//! Together, these modules form the foundation for reading, writing, and manipulating geometric data in the `versatiles_geometry` crate.

#![allow(clippy::module_inception)]

mod collection;
mod feature;
mod geometry;
mod properties;
mod types;
mod value;

pub use collection::*;
pub use feature::*;
pub use geometry::*;
pub use properties::*;
pub use types::*;
pub use value::*;
