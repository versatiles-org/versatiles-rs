use anyhow::Result;
use gdal::spatial_ref::{AxisMappingStrategy, SpatialRef};
use versatiles_derive::context;

#[context("Failed to get spatial reference for EPSG: {epsg}")]
pub fn get_spatial_ref(epsg: u32) -> Result<SpatialRef> {
	let mut srs = SpatialRef::from_epsg(epsg).map_err(|e| anyhow::anyhow!("Failed to get spatial reference: {e}"))?;
	srs.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
	Ok(srs)
}

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use gdal::spatial_ref::CoordTransform;

	use super::get_spatial_ref;

	// Reference values for (lon=10°, lat=50°) in EPSG:3857
	const R: f64 = 6_378_137.0; // WGS84 semi-major
	fn merc_x(lon_deg: f64) -> f64 {
		R * lon_deg.to_radians()
	}
	fn merc_y(lat_deg: f64) -> f64 {
		let phi = lat_deg.to_radians();
		R * ((std::f64::consts::FRAC_PI_4 + phi / 2.0).tan()).ln()
	}

	#[test]
	fn wgs84_to_mercator_axis_order_is_lonlat() -> Result<()> {
		let wgs84 = get_spatial_ref(4326)?; // Traditional GIS order enforced inside
		let merc = get_spatial_ref(3857)?;
		let ct = CoordTransform::new(&wgs84, &merc)?;

		let (lon, lat) = (10.0_f64, 50.0_f64);
		let mut coords = [lon, lat, 0.0];
		let (x_slice, rest) = coords.split_at_mut(1);
		let (y_slice, z_slice) = rest.split_at_mut(1);
		ct.transform_coords(x_slice, y_slice, z_slice)?;
		let (x, y) = (coords[0], coords[1]);

		let (x_exp, y_exp) = (merc_x(lon), merc_y(lat));
		assert!((x - x_exp).abs() < 5.0, "x mismatch: got {x}, want {x_exp}");
		assert!((y - y_exp).abs() < 5.0, "y mismatch: got {y}, want {y_exp}");
		Ok(())
	}

	#[test]
	fn roundtrip_wgs84_mercator() -> Result<()> {
		let wgs84 = get_spatial_ref(4326)?;
		let merc = get_spatial_ref(3857)?;
		let fwd = CoordTransform::new(&wgs84, &merc)?;
		let inv = CoordTransform::new(&merc, &wgs84)?;

		let (lon0, lat0) = (7.2_f64, 41.5_f64);
		let mut coords = [lon0, lat0, 0.0];
		let (x_slice, rest) = coords.split_at_mut(1);
		let (y_slice, z_slice) = rest.split_at_mut(1);
		fwd.transform_coords(x_slice, y_slice, z_slice)?;
		inv.transform_coords(x_slice, y_slice, z_slice)?;
		let (x, y) = (coords[0], coords[1]);

		assert!((x - lon0).abs() < 1e-8, "lon roundtrip mismatch: {x} vs {lon0}");
		assert!((y - lat0).abs() < 1e-8, "lat roundtrip mismatch: {y} vs {lat0}");
		Ok(())
	}

	/// `EpsgCode` is deliberately shallower than PROJ's register — it knows the
	/// range, not the contents — but it must not be *narrower*: a code GDAL
	/// resolves has to parse, or `check` would refuse a pipeline that builds.
	#[test]
	fn every_code_gdal_resolves_is_one_the_type_accepts() {
		for code in [2000, 3035, 3068, 3785, 3857, 4326, 25832, 32766] {
			assert!(get_spatial_ref(code).is_ok(), "EPSG:{code} should resolve");
			assert!(
				versatiles_core::EpsgCode::new(code).is_ok(),
				"GDAL resolves EPSG:{code} and the type refuses it"
			);
		}

		// The one documented exception. GDAL resolves the legacy Google spelling
		// of web mercator; the register has no such code, so the type refuses it
		// and the error names 3857 instead.
		assert!(get_spatial_ref(900_913).is_ok(), "GDAL still resolves the legacy code");
		let error = versatiles_core::EpsgCode::new(900_913).expect_err("not a register code");
		assert!(error.to_string().contains("use 3857"), "{error}");
	}

	#[test]
	fn invalid_epsg_yields_error() {
		// 0 is not a valid EPSG code; expect an error
		let res = get_spatial_ref(0);
		assert!(res.is_err(), "expected error for invalid EPSG code");
	}
}
