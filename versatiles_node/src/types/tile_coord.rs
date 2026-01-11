use crate::napi_result;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use versatiles_core::TileCoord as RustTileCoord;

/// Tile coordinate in the Web Mercator tile grid
///
/// Represents a specific tile in the standard Web Mercator (EPSG:3857) tiling scheme.
/// Uses XYZ coordinate convention where:
/// - **z** (zoom): Zoom level (0-32), where 0 is the world in one tile
/// - **x** (column): Tile column, from 0 (west) to 2^z - 1 (east)
/// - **y** (row): Tile row, from 0 (north) to 2^z - 1 (south)
///
/// # Coordinate System
///
/// This implementation uses the **XYZ** (Slippy Map) convention:
/// - Origin (0,0) is at the top-left (north-west)
/// - Y increases going south
/// - X increases going east
///
/// Note: This differs from **TMS** (Tile Map Service) where Y increases going north.
/// Use the `flipY` option in conversion if you need TMS coordinates.
#[napi]
pub struct TileCoord {
	inner: RustTileCoord,
}

#[napi]
impl TileCoord {
	/// Create a new tile coordinate
	///
	/// # Arguments
	///
	/// * `z` - Zoom level (0-32)
	/// * `x` - Tile column (0 to 2^z - 1)
	/// * `y` - Tile row (0 to 2^z - 1)
	///
	/// # Errors
	///
	/// Returns an error if coordinates are out of valid range for the zoom level.
	/// At zoom level z, valid coordinates are:
	/// - x: 0 to 2^z - 1
	/// - y: 0 to 2^z - 1
	///
	/// # Examples
	///
	/// ```javascript
	/// // Berlin tile at zoom 10
	/// const tile = new TileCoord(10, 550, 335);
	///
	/// // World tile at zoom 0
	/// const world = new TileCoord(0, 0, 0);
	/// ```
	#[napi(constructor)]
	pub fn new(z: u32, x: u32, y: u32) -> Result<Self> {
		if z > 30 {
			return Err(Error::from_reason("Zoom level must be between 0 and 30"));
		}
		#[allow(clippy::cast_possible_truncation)]
		let inner = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		Ok(Self { inner })
	}

	/// Create a tile coordinate from geographic coordinates
	///
	/// Converts WGS84 latitude/longitude to the tile containing that point.
	///
	/// # Arguments
	///
	/// * `lon` - Longitude in decimal degrees (-180 to 180)
	/// * `lat` - Latitude in decimal degrees (-85.0511 to 85.0511)
	/// * `z` - Zoom level (0-32)
	///
	/// # Returns
	///
	/// The tile coordinate containing the specified geographic point
	///
	/// # Errors
	///
	/// Returns an error if coordinates are outside valid Web Mercator range.
	/// Valid latitude range is approximately ±85.05° (Web Mercator limit).
	///
	/// # Examples
	///
	/// ```javascript
	/// // Find tile containing Berlin city center
	/// const berlin = TileCoord.fromGeo(13.405, 52.520, 10);
	/// console.log(`Tile: ${berlin.z}/${berlin.x}/${berlin.y}`);
	///
	/// // Find tile for New York City
	/// const nyc = TileCoord.fromGeo(-74.006, 40.7128, 12);
	/// ```
	#[napi(factory)]
	pub fn from_geo(lon: f64, lat: f64, z: u32) -> Result<Self> {
		if z > 30 {
			return Err(Error::from_reason("Zoom level must be between 0 and 30"));
		}
		#[allow(clippy::cast_possible_truncation)]
		let inner = napi_result!(RustTileCoord::from_geo(lon, lat, z as u8))?;
		Ok(Self { inner })
	}

	/// Get the geographic center point of this tile
	///
	/// Returns the WGS84 coordinates of the tile's center point.
	///
	/// # Returns
	///
	/// Array of `[longitude, latitude]` in decimal degrees
	///
	/// # Examples
	///
	/// ```javascript
	/// const tile = new TileCoord(10, 550, 335);
	/// const [lon, lat] = tile.toGeo();
	/// console.log(`Center: ${lat.toFixed(4)}°N, ${lon.toFixed(4)}°E`);
	/// ```
	#[napi]
	pub fn to_geo(&self) -> Vec<f64> {
		let [lon, lat] = self.inner.as_geo();
		vec![lon, lat]
	}

	/// Get the geographic bounding box of this tile
	///
	/// Returns the geographic extent of the tile in WGS84 coordinates.
	///
	/// # Returns
	///
	/// Array of `[west, south, east, north]` in decimal degrees:
	/// - **west**: Minimum longitude (left edge)
	/// - **south**: Minimum latitude (bottom edge)
	/// - **east**: Maximum longitude (right edge)
	/// - **north**: Maximum latitude (top edge)
	///
	/// # Examples
	///
	/// ```javascript
	/// const tile = new TileCoord(10, 550, 335);
	/// const [west, south, east, north] = tile.toGeoBbox();
	/// console.log(`Bbox: ${west},${south},${east},${north}`);
	/// ```
	#[napi]
	pub fn to_geo_bbox(&self) -> Vec<f64> {
		self.inner.to_geo_bbox().as_array().to_vec()
	}

	/// Get the zoom level (0-32)
	///
	/// Returns the tile's zoom level. At zoom 0, the entire world is one tile.
	/// Each zoom level doubles the number of tiles in each dimension.
	#[napi(getter)]
	pub fn z(&self) -> u32 {
		self.inner.level as u32
	}

	/// Get the tile column (x coordinate)
	///
	/// Returns the horizontal position in the tile grid (0 to 2^z - 1).
	/// Lower values are further west, higher values are further east.
	#[napi(getter)]
	pub fn x(&self) -> u32 {
		self.inner.x
	}

	/// Get the tile row (y coordinate)
	///
	/// Returns the vertical position in the tile grid (0 to 2^z - 1).
	/// In XYZ convention: lower values are further north, higher values are further south.
	#[napi(getter)]
	pub fn y(&self) -> u32 {
		self.inner.y
	}

	/// Convert to JSON string representation
	///
	/// Returns a JSON object with z, x, and y properties.
	///
	/// # Examples
	///
	/// ```javascript
	/// const tile = new TileCoord(10, 550, 335);
	/// console.log(tile.toJson());  // '{"z":10,"x":550,"y":335}'
	/// ```
	#[napi]
	pub fn to_json(&self) -> String {
		self.inner.as_json()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[rstest]
	#[case(5, 10, 15)]
	#[case(0, 0, 0)]
	#[case(10, 512, 512)]
	#[case(15, 16384, 16384)]
	#[case(20, 524287, 524287)]
	fn test_new_creates_valid_tile_coord(#[case] z: u32, #[case] x: u32, #[case] y: u32) {
		let coord = TileCoord::new(z, x, y).unwrap();
		assert_eq!(coord.z(), z);
		assert_eq!(coord.x(), x);
		assert_eq!(coord.y(), y);
	}

	#[test]
	fn test_new_with_invalid_coordinates_fails() {
		let result = TileCoord::new(2, 10, 0);
		assert!(result.is_err());
	}

	#[rstest]
	#[case(0.0, 0.0, 0, 0, 0)]
	#[case(-180.0, 85.0511, 1, 0, 0)]
	#[case(180.0, -85.0511, 1, 1, 1)]
	#[case(-77.0365, 38.8977, 5, 9, 12)] // Washington, D.C.
	#[case(2.3522, 48.8566, 7, 64, 44)] // Paris
	#[case(151.2093, -33.8688, 12, 3768, 2457)] // Sydney
	#[case(13.405, 52.520, 10, 550, 335)] // Berlin
	#[case(-74.006, 40.7128, 8, 75, 96)] // New York
	#[case(139.6917, 35.6895, 12, 3637, 1612)] // Tokyo
	fn test_from_geo(#[case] lon: f64, #[case] lat: f64, #[case] z: u32, #[case] x: u32, #[case] y: u32) {
		let coord = TileCoord::from_geo(lon, lat, z).unwrap();
		assert_eq!(coord.z(), z);
		assert_eq!(coord.x(), x);
		assert_eq!(coord.y(), y);
	}

	#[test]
	fn test_to_geo_returns_center_coordinates() {
		let coord = TileCoord::new(10, 550, 335).unwrap();
		let geo = coord.to_geo();

		assert_eq!(geo.len(), 2);
		// Should return [longitude, latitude] near Berlin
		assert!(geo[0] > 13.0 && geo[0] < 14.0); // lon
		assert!(geo[1] > 52.0 && geo[1] < 53.0); // lat
	}

	#[test]
	fn test_to_geo_at_zoom_zero() {
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let geo = coord.to_geo();

		assert_eq!(geo.len(), 2);
		// Zoom 0 tile (0,0) covers the entire world, center is at western edge and near north pole
		assert_eq!(geo[0], -180.0);
		assert!((geo[1] - 85.05).abs() < 0.1); // Near the Web Mercator northern limit
	}

	#[test]
	fn test_to_geo_bbox_returns_four_values() {
		let coord = TileCoord::new(5, 10, 15).unwrap();
		let bbox = coord.to_geo_bbox();

		assert_eq!(bbox.len(), 4);
		// Should return [west, south, east, north]
		assert!(bbox[0] < bbox[2]); // west < east
		assert!(bbox[1] < bbox[3]); // south < north
	}

	#[test]
	fn test_to_geo_bbox_at_zoom_zero_covers_world() {
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let bbox = coord.to_geo_bbox();

		assert_eq!(bbox.len(), 4);
		// At zoom 0, bbox should cover the full Web Mercator extent
		assert!(bbox[0] >= -180.0); // west
		assert!(bbox[1] >= -90.0); // south (can be slightly beyond -85.05 Web Mercator limit)
		assert!(bbox[2] <= 180.0); // east
		assert!(bbox[3] <= 90.0); // north (can be slightly beyond 85.05 Web Mercator limit)
	}

	#[test]
	fn test_to_geo_bbox_higher_zoom_smaller_area() {
		let coord_low = TileCoord::new(1, 0, 0).unwrap();
		let coord_high = TileCoord::new(5, 0, 0).unwrap();

		let bbox_low = coord_low.to_geo_bbox();
		let bbox_high = coord_high.to_geo_bbox();

		// Higher zoom should have smaller area
		let width_low = bbox_low[2] - bbox_low[0];
		let width_high = bbox_high[2] - bbox_high[0];
		assert!(width_high < width_low);
	}

	#[test]
	fn test_getters_return_correct_values() {
		let coord = TileCoord::new(7, 42, 99).unwrap();

		assert_eq!(coord.z(), 7);
		assert_eq!(coord.x(), 42);
		assert_eq!(coord.y(), 99);
	}

	#[rstest]
	#[case(10, 550, 335, "{\"z\":10,\"x\":550,\"y\":335}")]
	#[case(0, 0, 0, "{\"z\":0,\"x\":0,\"y\":0}")]
	#[case(15, 16384, 16384, "{\"z\":15,\"x\":16384,\"y\":16384}")]
	#[case(20, 524287, 524287, "{\"z\":20,\"x\":524287,\"y\":524287}")]
	fn test_to_json(#[case] z: u32, #[case] x: u32, #[case] y: u32, #[case] expected_json: &str) {
		let coord = TileCoord::new(z, x, y).unwrap();
		assert_eq!(coord.to_json(), expected_json);
	}

	#[test]
	fn test_roundtrip_geo_conversion_approximate() {
		// Create from geo coordinates
		let lon = 13.405;
		let lat = 52.520;
		let zoom = 15;

		let coord = TileCoord::from_geo(lon, lat, zoom).unwrap();
		let geo_back = coord.to_geo();

		// Should be close to original (within tile precision)
		assert!((geo_back[0] - lon).abs() < 0.01);
		assert!((geo_back[1] - lat).abs() < 0.01);
	}

	#[test]
	fn test_edge_case_international_date_line() {
		// Test coordinates near international date line (lon ~180/-180)
		let coord_east = TileCoord::from_geo(179.9, 0.0, 5).unwrap();
		let coord_west = TileCoord::from_geo(-179.9, 0.0, 5).unwrap();

		assert_eq!(coord_east.z(), 5);
		assert_eq!(coord_west.z(), 5);
		// Should be different x coordinates
		assert_ne!(coord_east.x(), coord_west.x());
	}

	#[test]
	fn test_edge_case_north_pole() {
		// Test coordinates near north pole (max lat in Web Mercator ~85.05)
		let coord = TileCoord::from_geo(0.0, 85.0, 8).unwrap();
		assert_eq!(coord.z(), 8);
		assert_eq!(coord.y(), 0); // North pole should be at y=0
	}

	#[test]
	fn test_edge_case_south_pole() {
		// Test coordinates near south pole (min lat in Web Mercator ~-85.05)
		let coord = TileCoord::from_geo(0.0, -85.0, 8).unwrap();
		assert_eq!(coord.z(), 8);
		assert_eq!(coord.y(), 255); // South pole should be at max y for zoom 8
	}
}
