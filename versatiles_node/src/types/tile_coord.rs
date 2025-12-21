use crate::napi_result;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use versatiles_core::TileCoord as RustTileCoord;

/// Tile coordinate with zoom level (z), column (x), and row (y)
#[napi]
pub struct TileCoord {
	inner: RustTileCoord,
}

#[napi]
impl TileCoord {
	/// Create a new TileCoord
	#[napi(constructor)]
	pub fn new(z: u32, x: u32, y: u32) -> Result<Self> {
		let inner = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		Ok(Self { inner })
	}

	/// Create a TileCoord from geographic coordinates
	#[napi(factory)]
	pub fn from_geo(lon: f64, lat: f64, z: u32) -> Result<Self> {
		let inner = napi_result!(RustTileCoord::from_geo(lon, lat, z as u8))?;
		Ok(Self { inner })
	}

	/// Convert to geographic coordinates [longitude, latitude]
	#[napi]
	pub fn to_geo(&self) -> Vec<f64> {
		let [lon, lat] = self.inner.as_geo();
		vec![lon, lat]
	}

	/// Get the geographic bounding box [west, south, east, north]
	#[napi]
	pub fn to_geo_bbox(&self) -> Vec<f64> {
		self.inner.to_geo_bbox().as_array().to_vec()
	}

	/// Get the zoom level
	#[napi(getter)]
	pub fn z(&self) -> u32 {
		self.inner.level as u32
	}

	/// Get the column (x)
	#[napi(getter)]
	pub fn x(&self) -> u32 {
		self.inner.x
	}

	/// Get the row (y)
	#[napi(getter)]
	pub fn y(&self) -> u32 {
		self.inner.y
	}

	/// Get JSON representation
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
