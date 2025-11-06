//! This module provides functionality for parsing and reading GeoJSON data.
//!
//! It includes:
//! - [`parse`]: low-level functions for parsing GeoJSON strings into internal geometry representations.
//! - [`read`]: higher-level functions for reading full GeoJSON or newline-delimited GeoJSON (NDGeoJSON) files.
//!
//! Together, these modules form the GeoJSON interface of the `versatiles_geometry` crate,
//! used for converting GeoJSON inputs into the crate’s geometry types such as [`GeoCollection`], [`GeoFeature`], [`Geometry`], and others.

mod parse;
mod read;

pub use parse::*;
pub use read::*;
