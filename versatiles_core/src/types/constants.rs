//! Geographic and Web Mercator constants used across the VersaTiles project.

use std::f64::consts::PI;

/// WGS84 semi-major axis (equatorial radius) in meters.
pub const EARTH_RADIUS: f64 = 6_378_137.0;

/// Earth circumference in meters at the equator (2 * PI * EARTH_RADIUS).
pub const WORLD_SIZE: f64 = 2.0 * PI * EARTH_RADIUS;

/// Maximum latitude in degrees for the Web Mercator projection (EPSG:3857).
///
/// Equals `atan(sinh(PI))` in degrees. Coordinates beyond this are clamped
/// when projecting to/from Mercator.
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

/// Maximum longitude in degrees for the Web Mercator projection (EPSG:3857).
pub const MAX_LON: f64 = 180.0;
