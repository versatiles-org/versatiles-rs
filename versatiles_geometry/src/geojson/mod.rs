//! This module provides functionality for parsing and reading GeoJSON data.
//!
//! Together, these modules form the GeoJSON interface of the `versatiles_geometry` crate,
//! used for converting GeoJSON inputs into the crate's geometry types such as [`crate::geo::GeoCollection`], [`crate::geo::GeoFeature`], and `geo_types::Geometry<f64>`.

mod parse;
mod read;

pub use parse::*;
pub use read::*;
