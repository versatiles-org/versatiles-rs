//! Extension traits and free-function helpers operating on `geo_types` geometries.
//!
//! Versatiles-specific concerns (Web Mercator projection, GeoJSON output, structural
//! validation) live here as standalone helpers so the geometry types stay vanilla
//! `geo_types` and the georust ecosystem plugs in without conversion.

pub mod geojson_io;
pub mod mercator;
pub mod validate;

pub use geojson_io::{coord_to_json, geometry_to_json, type_name};
pub use mercator::{MercatorExt, coord_from_mercator, coord_to_mercator};
pub use validate::validate;
