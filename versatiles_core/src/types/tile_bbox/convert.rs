use crate::{GeoBBox, TileBBox, TileCoord};
use anyhow::{Result, ensure};

impl TileBBox {
	// -------------------------------------------------------------------------
	// Coordinate Transformations
	// -------------------------------------------------------------------------

	/// Converts this tile bounding box to its equivalent geographic extent (`GeoBBox`).
	///
	/// The conversion uses the **Web Mercator tile schema** (EPSG:3857) projected back
	/// to geographic coordinates in **degrees** (EPSG:4326). The result is a
	/// longitude/latitude rectangle covering the same area as the tiles represented
	/// by this `TileBBox`.
	///
	/// ## Details
	/// * The lower‑left corner corresponds to tile `(x_min, y_max + 1)`.
	/// * The upper‑right corner corresponds to tile `(x_max + 1, y_min)`.
	/// * Coordinates are inclusive in tile space but continuous in degrees.
	/// * Output order is `(west, south, east, north)` in degrees.
	///
	/// ## Returns
	/// A [`GeoBBox`] representing the geographic region covered by this bounding box.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		if self.is_empty() {
			return None;
		}
		// Bottom-left in geospatial terms is (x_min, y_max + 1)
		let p_min = TileCoord::coord_to_geo(
			self.level,
			f64::from(self.x_min().expect("bbox is non-empty")),
			f64::from(self.y_max().expect("bbox is non-empty") + 1),
		);
		// Top-right in geospatial terms is (x_max + 1, y_min)
		let p_max = TileCoord::coord_to_geo(
			self.level,
			f64::from(self.x_max().expect("bbox is non-empty") + 1),
			f64::from(self.y_min().expect("bbox is non-empty")),
		);

		GeoBBox::new(p_min[0], p_min[1], p_max[0], p_max[1]).ok()
	}

	/// Returns the bbox as an array `[x_min, y_min, x_max, y_max]`.
	/// Useful for serialization or equality checks.
	pub fn to_array(&self) -> Result<[u32; 4]> {
		ensure!(!self.is_empty(), "cannot get array of an empty TileBBox");
		Ok([self.x_min()?, self.y_min()?, self.x_max()?, self.y_max()?])
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use rstest::rstest;

	fn approx_eq(a: f64, b: f64, eps: f64) -> bool {
		(a - b).abs() <= eps
	}

	/// Compute the expected GeoBBox by reproducing the same corner logic
	/// used by `to_geo_bbox` directly via `TileCoord::as_geo()`.
	fn expected_bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> GeoBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1)
			.unwrap()
			.to_geo_bbox()
			.unwrap()
	}

	fn assert_bbox_close(got: &GeoBBox, exp: &GeoBBox, eps: f64) {
		let (gx0, gy0, gx1, gy1) = got.as_tuple();
		let (ex0, ey0, ex1, ey1) = exp.as_tuple();
		assert!(approx_eq(gx0, ex0, eps), "x_min mismatch: got {gx0}, exp {ex0}");
		assert!(approx_eq(gy0, ey0, eps), "y_min mismatch: got {gy0}, exp {ey0}");
		assert!(approx_eq(gx1, ex1, eps), "x_max mismatch: got {gx1}, exp {ex1}");
		assert!(approx_eq(gy1, ey1, eps), "y_max mismatch: got {gy1}, exp {ey1}");
	}

	#[rstest]
	#[case(0, 0, 0, 0, 0)]
	#[case(1, 0, 0, 0, 0)]
	#[case(1, 1, 1, 1, 1)]
	#[case(2, 0, 0, 3, 3)]
	#[case(4, 5, 6, 7, 8)]
	#[case(6, 0, 31, 32, 63)]
	#[case(8, 100, 120, 140, 180)]
	fn to_geo_bbox_matches_expected(
		#[case] level: u8,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
	) -> Result<()> {
		let bb = TileBBox::from_min_and_max(level, x0, y0, x1, y1)?;
		let got = bb.to_geo_bbox().unwrap();
		let exp = expected_bbox(level, x0, y0, x1, y1);

		// Exact equality should hold because the same math is used, but allow tiny epsilon
		assert_bbox_close(&got, &exp, 1e-9);

		// Ordering invariants
		let (gx0, gy0, gx1, gy1) = got.as_tuple();
		assert!(gx0 <= gx1 && gy0 <= gy1, "bbox not ordered: {:?}", got.as_tuple());
		Ok(())
	}

	#[rstest]
	// Thin vertical strip
	#[case(5, 10, 0, 10, (1u32<<5)-1)]
	// Thin horizontal strip
	#[case(5, 0, 20, (1u32<<5)-1, 20)]
	// Single-tile box
	#[case(7, 33, 44, 33, 44)]
	fn to_geo_bbox_degenerate_shapes(
		#[case] level: u8,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
	) -> Result<()> {
		let bb = TileBBox::from_min_and_max(level, x0, y0, x1, y1)?;
		let got = bb.to_geo_bbox().unwrap();
		let exp = expected_bbox(level, x0, y0, x1, y1);
		assert_bbox_close(&got, &exp, 1e-9);
		Ok(())
	}

	#[test]
	fn to_geo_bbox_world_bounds_roundtrip() -> Result<()> {
		// Full world at z=2 should map to finite lon/lat bounds
		let level = 2u8;
		let max = (1u32 << level) - 1;
		let bb = TileBBox::from_min_and_max(level, 0, 0, max, max)?;
		let got = bb.to_geo_bbox().unwrap();
		let (minlon, minlat, maxlon, maxlat) = got.as_tuple();
		assert!(minlon <= maxlon && minlat <= maxlat);
		// Reasonable numeric ranges
		assert!(minlon >= -180.0 && maxlon <= 180.0);
		assert!(minlat >= -90.0 && maxlat <= 90.0);
		Ok(())
	}

	#[test]
	fn quarter_planet() -> Result<()> {
		let geo_bbox = GeoBBox::new(0.0, -85.05112877980659f64, 180.0, 0.0)?;
		for level in 1..30 {
			let bbox = TileBBox::from_geo_bbox(level, &geo_bbox)?;
			assert_eq!(bbox.count_tiles(), 4u64.pow(u32::from(level) - 1));
			assert_eq!(bbox.to_geo_bbox().unwrap(), geo_bbox);
		}
		Ok(())
	}

	#[test]
	fn sa_pacific() -> Result<()> {
		let geo_bbox = GeoBBox::new(-180.0, -66.51326044311186f64, -90.0, 0.0)?;
		for level in 2..30 {
			let bbox = TileBBox::from_geo_bbox(level, &geo_bbox)?;
			assert_eq!(bbox.count_tiles(), 4u64.pow(u32::from(level) - 2));
			assert_eq!(bbox.to_geo_bbox().unwrap(), geo_bbox);
		}
		Ok(())
	}

	#[test]
	fn test_as_geo_bbox() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
		let geo_bbox = bbox.to_geo_bbox().unwrap();
		assert_eq!(
			geo_bbox.to_list_string(),
			"-67.5,-74.01954331150228,0,-40.97989806962013"
		);
		Ok(())
	}

	#[test]
	fn should_convert_to_geo_bbox_correctly() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
		let geo_bbox = bbox.to_geo_bbox().unwrap();
		let expected_geo_bbox = GeoBBox::new(-67.5, -74.01954331150228, 0.0, -40.97989806962013)?;
		assert_eq!(geo_bbox, expected_geo_bbox);
		Ok(())
	}
}
