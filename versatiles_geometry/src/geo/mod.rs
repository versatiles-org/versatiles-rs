//! Feature/property layer of the `versatiles_geometry` crate.
//!
//! This module exposes [`GeoFeature`], [`GeoCollection`], [`GeoProperties`], and
//! [`GeoValue`]. Geometry primitives themselves come from `geo_types` (re-exported
//! from this crate's root); helpers in [`crate::ext`] provide projection, GeoJSON
//! output, and validation over those primitives.

#![allow(clippy::module_inception)]

mod collection;
mod feature;
mod properties;
mod value;

pub use collection::*;
pub use feature::*;
pub use properties::*;
pub use value::*;
