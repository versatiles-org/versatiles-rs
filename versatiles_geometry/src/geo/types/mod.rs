// This module defines the core geometric types used throughout the `versatiles_geometry` crate.
// It includes basic primitives such as `PointGeometry`, `LineStringGeometry`, `PolygonGeometry`,
// and their multi-geometry counterparts. These types implement shared traits like `GeometryTrait`,
// `SingleGeometryTrait`, and `CompositeGeometryTrait` to provide consistent behavior across
// geometry types, including validation, area calculation, and JSON conversion.
// The module re-exports all geometry types for convenient public access.

mod coordinates;
mod linestring;
mod macros;
mod multi_linestring;
mod multi_point;
mod multi_polygon;
mod point;
mod polygon;
mod ring;
mod traits;

pub use coordinates::*;
pub use linestring::*;
pub use multi_linestring::*;
pub use multi_point::*;
pub use multi_polygon::*;
pub use point::*;
pub use polygon::*;
pub use ring::*;
pub use traits::*;
