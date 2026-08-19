//! Projections a grid can be defined in, without needing GDAL.
//!
//! A grid source has to move between its own CRS — where cells are squares of a
//! fixed size — and web mercator, where tiles are cut. GDAL can do that for any
//! EPSG code, but releases ship without it (see #232), so the codes that matter
//! most for gridded statistics are implemented here directly:
//!
//! | EPSG | | |
//! |---|---|---|
//! | 4326 | WGS84 lon/lat | cells sized in degrees |
//! | 3857 | Web Mercator | the identity, cells sized in mercator meters |
//! | 3035 | ETRS89-LAEA | what European gridded statistics are published in |
//!
//! Anything else is an error naming these three. When the `gdal` feature grows
//! a projection of its own, it slots in behind the same trait.

use std::fmt::Debug;

use anyhow::{Result, bail};
use geo_types::{Coord, coord};
use versatiles_geometry::ext::mercator::{coord_from_mercator, coord_to_mercator};

/// Converts between a grid's own CRS and web mercator meters.
pub trait Projection: Debug + Send + Sync {
	/// A coordinate in the grid's CRS, as web mercator meters.
	fn to_mercator(&self, c: Coord<f64>) -> Coord<f64>;

	/// A web mercator coordinate, in the grid's CRS.
	fn to_source(&self, c: Coord<f64>) -> Coord<f64>;
}

/// The projection for an EPSG code, or an error naming what is available.
pub fn for_epsg(epsg: u32) -> Result<Box<dyn Projection>> {
	Ok(match epsg {
		4326 => Box::new(Wgs84) as Box<dyn Projection>,
		3857 => Box::new(WebMercator) as Box<dyn Projection>,
		3035 => Box::new(Laea::etrs89()) as Box<dyn Projection>,
		other => bail!(
			"EPSG:{other} is not supported for grids. Available without GDAL: 4326 (WGS84 lon/lat), 3857 (web \
			 mercator) and 3035 (ETRS89-LAEA, what European gridded statistics use)."
		),
	})
}

/// WGS84 lon/lat in degrees.
#[derive(Debug)]
pub struct Wgs84;

impl Projection for Wgs84 {
	fn to_mercator(&self, c: Coord<f64>) -> Coord<f64> {
		coord_to_mercator(c)
	}

	fn to_source(&self, c: Coord<f64>) -> Coord<f64> {
		coord_from_mercator(c)
	}
}

/// Web mercator meters — the space tiles are already cut in.
#[derive(Debug)]
pub struct WebMercator;

impl Projection for WebMercator {
	fn to_mercator(&self, c: Coord<f64>) -> Coord<f64> {
		c
	}

	fn to_source(&self, c: Coord<f64>) -> Coord<f64> {
		c
	}
}

/// Ellipsoidal Lambert Azimuthal Equal-Area, oblique aspect.
///
/// Snyder, *Map Projections — A Working Manual*, pp. 187–190. The authalic
/// latitude series in the inverse is his (3-18), good to well under a
/// millimeter at these scales.
#[derive(Debug)]
pub struct Laea {
	/// Eccentricity squared.
	e2: f64,
	/// Latitude of origin, radians.
	lat0: f64,
	/// Longitude of origin, radians.
	lon0: f64,
	false_easting: f64,
	false_northing: f64,
	// Derived once, since every cell corner uses them.
	qp: f64,
	rq: f64,
	beta0: f64,
	d: f64,
}

impl Laea {
	/// EPSG:3035 — ETRS89-extended / LAEA Europe, on GRS80.
	#[must_use]
	pub fn etrs89() -> Self {
		const A: f64 = 6_378_137.0;
		const INV_F: f64 = 298.257_222_101;
		let f = 1.0 / INV_F;
		let e2 = f.mul_add(2.0, -(f * f));
		Self::new(A, e2, 52f64.to_radians(), 10f64.to_radians(), 4_321_000.0, 3_210_000.0)
	}

	fn new(a: f64, e2: f64, lat0: f64, lon0: f64, false_easting: f64, false_northing: f64) -> Self {
		let e = e2.sqrt();
		let qp = authalic_q(FRAC_PI_2_SIN, e, e2);
		let q0 = authalic_q(lat0.sin(), e, e2);
		let beta0 = (q0 / qp).asin();
		let rq = a * (qp / 2.0).sqrt();
		let d = a * lat0.cos() / ((1.0 - e2 * lat0.sin().powi(2)).sqrt() * rq * beta0.cos());

		Self {
			e2,
			lat0,
			lon0,
			false_easting,
			false_northing,
			qp,
			rq,
			beta0,
			d,
		}
	}

	/// Projected coordinates for a lon/lat in degrees.
	fn forward(&self, lon_deg: f64, lat_deg: f64) -> Coord<f64> {
		let lat = lat_deg.to_radians();
		let lon = lon_deg.to_radians();
		let e = self.e2.sqrt();

		let beta = (authalic_q(lat.sin(), e, self.e2) / self.qp).asin();
		let dlon = wrap_radians(lon - self.lon0);

		let denominator = 1.0 + self.beta0.sin() * beta.sin() + self.beta0.cos() * beta.cos() * dlon.cos();
		// Antipodal to the origin: the projection is undefined there, and a grid
		// reaching that far is a mistake elsewhere. Keep it finite rather than NaN.
		let b = if denominator <= f64::EPSILON {
			self.rq * 2f64.sqrt()
		} else {
			self.rq * (2.0 / denominator).sqrt()
		};

		coord! {
			x: self.false_easting + b * self.d * beta.cos() * dlon.sin(),
			y: self.false_northing + (b / self.d) * (self.beta0.cos() * beta.sin() - self.beta0.sin() * beta.cos() * dlon.cos()),
		}
	}

	/// Lon/lat in degrees for projected coordinates.
	fn inverse(&self, c: Coord<f64>) -> Coord<f64> {
		let x = (c.x - self.false_easting) / self.d;
		let y = (c.y - self.false_northing) * self.d;
		let rho = x.hypot(y);

		if rho < f64::EPSILON {
			// The origin itself, where the azimuth is undefined.
			return coord! { x: self.lon0.to_degrees(), y: self.lat0.to_degrees() };
		}

		let c_angle = 2.0 * (rho / (2.0 * self.rq)).clamp(-1.0, 1.0).asin();
		let (sin_c, cos_c) = c_angle.sin_cos();

		let beta = ((cos_c * self.beta0.sin()) + (y * sin_c * self.beta0.cos() / rho))
			.clamp(-1.0, 1.0)
			.asin();
		let lon = self.lon0 + (x * sin_c).atan2(rho * self.beta0.cos() * cos_c - y * self.beta0.sin() * sin_c);

		coord! { x: wrap_radians(lon).to_degrees(), y: authalic_to_geodetic(beta, self.e2).to_degrees() }
	}
}

/// `sin(π/2)`, spelled out because `q` is evaluated at the pole to get `qp`.
const FRAC_PI_2_SIN: f64 = 1.0;

/// Snyder (3-12): the authalic area function `q`.
fn authalic_q(sin_lat: f64, e: f64, e2: f64) -> f64 {
	if e < f64::EPSILON {
		return 2.0 * sin_lat;
	}
	let esin = e * sin_lat;
	(1.0 - e2) * (sin_lat / (1.0 - esin * esin) - (1.0 / (2.0 * e)) * ((1.0 - esin) / (1.0 + esin)).ln())
}

/// Snyder (3-18): geodetic latitude from authalic latitude, as a series in `e²`.
fn authalic_to_geodetic(beta: f64, e2: f64) -> f64 {
	let e4 = e2 * e2;
	let e6 = e4 * e2;
	beta
		+ (e2 / 3.0 + 31.0 * e4 / 180.0 + 517.0 * e6 / 5040.0) * (2.0 * beta).sin()
		+ (23.0 * e4 / 360.0 + 251.0 * e6 / 3780.0) * (4.0 * beta).sin()
		+ (761.0 * e6 / 45360.0) * (6.0 * beta).sin()
}

/// Fold an angle into (-π, π].
fn wrap_radians(mut angle: f64) -> f64 {
	use std::f64::consts::{PI, TAU};
	while angle > PI {
		angle -= TAU;
	}
	while angle < -PI {
		angle += TAU;
	}
	angle
}

impl Projection for Laea {
	fn to_mercator(&self, c: Coord<f64>) -> Coord<f64> {
		coord_to_mercator(self.inverse(c))
	}

	fn to_source(&self, c: Coord<f64>) -> Coord<f64> {
		let lonlat = coord_from_mercator(c);
		self.forward(lonlat.x, lonlat.y)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Reference values from PROJ 9.8.1 (`cs2cs EPSG:3035 EPSG:4326`), which is
	/// what publishers of EPSG:3035 grids use.
	const REFERENCES: &[(f64, f64, f64, f64)] = &[
		// easting, northing, lon, lat
		(4_321_000.0, 3_210_000.0, 10.0, 52.0),
		(4_341_000.0, 2_691_000.0, 10.264_413_67, 47.332_274_49),
		(4_551_801.973_3, 3_271_028.983_2, 13.4, 52.5),
	];

	#[test]
	fn laea_matches_proj() {
		let laea = Laea::etrs89();
		for &(easting, northing, lon, lat) in REFERENCES {
			let projected = laea.forward(lon, lat);
			assert!(
				(projected.x - easting).abs() < 0.01 && (projected.y - northing).abs() < 0.01,
				"forward({lon}, {lat}) = {projected:?}, expected ({easting}, {northing})"
			);

			let geographic = laea.inverse(coord! { x: easting, y: northing });
			assert!(
				(geographic.x - lon).abs() < 1e-7 && (geographic.y - lat).abs() < 1e-7,
				"inverse({easting}, {northing}) = {geographic:?}, expected ({lon}, {lat})"
			);
		}
	}

	#[test]
	fn laea_round_trips_through_mercator() {
		let laea = Laea::etrs89();
		for x in [3_000_000.0, 4_321_000.0, 5_500_000.0] {
			for y in [1_800_000.0, 3_210_000.0, 4_400_000.0] {
				let source = coord! { x: x, y: y };
				let back = laea.to_source(laea.to_mercator(source));
				assert!(
					(back.x - source.x).abs() < 0.001 && (back.y - source.y).abs() < 0.001,
					"{source:?} came back as {back:?}"
				);
			}
		}
	}

	#[test]
	fn web_mercator_is_the_identity() {
		let c = coord! { x: 1_234_567.0, y: -7_654_321.0 };
		assert_eq!(WebMercator.to_mercator(c), c);
		assert_eq!(WebMercator.to_source(c), c);
	}

	#[test]
	fn wgs84_round_trips() {
		let c = coord! { x: 13.4, y: 52.5 };
		let back = Wgs84.to_source(Wgs84.to_mercator(c));
		assert!((back.x - c.x).abs() < 1e-9 && (back.y - c.y).abs() < 1e-9);
	}

	#[test]
	fn unsupported_codes_name_the_supported_ones() {
		let err = for_epsg(28992).unwrap_err().to_string();
		assert!(err.contains("28992"), "{err}");
		assert!(
			err.contains("4326") && err.contains("3857") && err.contains("3035"),
			"{err}"
		);
	}
}
