//! Any EPSG code, by way of GDAL.
//!
//! The projections written out in [`super::projection`] cover the codes gridded
//! statistics are usually published in, and they work in every build. This picks
//! up everything else — EPSG:28992 for Dutch grid statistics, EPSG:3067 for
//! Finnish ones — but only where GDAL is available, which released binaries are
//! not (see #232).
//!
//! It is also much slower: every corner crosses the FFI boundary on its own, and
//! generating a grid this way runs roughly two orders of magnitude below the
//! native projections. Fine for a one-off conversion, worth knowing before
//! reaching for it on a continent.

use std::{cell::RefCell, collections::HashMap};

use anyhow::Result;
use gdal::spatial_ref::CoordTransform;
use geo_types::{Coord, coord};

use super::projection::Projection;
use crate::operations::read::from_gdal::spatial_ref::get_spatial_ref;

/// Web mercator: the space tiles are cut in.
const MERCATOR_EPSG: u32 = 3857;

/// A projection backed by GDAL's transformation machinery.
///
/// Holds nothing but the EPSG code. A `CoordTransform` is neither `Send` nor
/// safe to share, while tiles are built in parallel, so the transforms live in
/// thread-local storage and each worker builds its own on first use.
#[derive(Debug)]
pub struct GdalProjection {
	epsg: u32,
}

impl GdalProjection {
	/// Fails here rather than on the first tile if GDAL cannot use the code.
	pub fn new(epsg: u32) -> Result<Self> {
		Transforms::build(epsg)?;
		Ok(Self { epsg })
	}

	fn with<T>(&self, f: impl FnOnce(&Transforms) -> T) -> T {
		TRANSFORMS.with_borrow_mut(|cache| {
			let transforms = cache
				.entry(self.epsg)
				// Unreachable: `new` built the same pair successfully.
				.or_insert_with(|| Transforms::build(self.epsg).expect("EPSG code was accepted when the grid was built"));
			f(transforms)
		})
	}
}

impl Projection for GdalProjection {
	fn to_mercator(&self, c: Coord<f64>) -> Coord<f64> {
		self.with(|transforms| apply(&transforms.to_mercator, c))
	}

	fn to_source(&self, c: Coord<f64>) -> Coord<f64> {
		self.with(|transforms| apply(&transforms.to_source, c))
	}

	fn to_mercator_many(&self, coords: &mut [Coord<f64>]) {
		if coords.is_empty() {
			return;
		}
		// Worth about 15% over one call per coordinate — PROJ's per-point work
		// dominates, not the crossing. The real saving is in asking for fewer
		// points, which is what the caller does.
		let mut xs: Vec<f64> = coords.iter().map(|c| c.x).collect();
		let mut ys: Vec<f64> = coords.iter().map(|c| c.y).collect();
		let mut zs = vec![0.0; coords.len()];

		let ok = self.with(|transforms| {
			transforms
				.to_mercator
				.transform_coords(&mut xs, &mut ys, &mut zs)
				.is_ok()
		});

		for (i, c) in coords.iter_mut().enumerate() {
			*c = if ok {
				coord! { x: xs[i], y: ys[i] }
			} else {
				coord! { x: f64::NAN, y: f64::NAN }
			};
		}
	}
}

/// Both directions between one CRS and web mercator.
struct Transforms {
	to_mercator: CoordTransform,
	to_source: CoordTransform,
}

impl Transforms {
	fn build(epsg: u32) -> Result<Self> {
		let source = get_spatial_ref(epsg)?;
		let mercator = get_spatial_ref(MERCATOR_EPSG)?;
		Ok(Self {
			to_mercator: CoordTransform::new(&source, &mercator)
				.map_err(|e| anyhow::anyhow!("EPSG:{epsg} cannot be transformed to web mercator: {e}"))?,
			to_source: CoordTransform::new(&mercator, &source)
				.map_err(|e| anyhow::anyhow!("web mercator cannot be transformed to EPSG:{epsg}: {e}"))?,
		})
	}
}

thread_local! {
	static TRANSFORMS: RefCell<HashMap<u32, Transforms>> = RefCell::new(HashMap::new());
}

/// One coordinate through one transform.
///
/// A point outside the CRS's area of use has no image; that comes back as `NaN`,
/// which the caller drops along with the cell it belongs to rather than drawing
/// a cell in the wrong place.
fn apply(transform: &CoordTransform, c: Coord<f64>) -> Coord<f64> {
	let mut x = [c.x];
	let mut y = [c.y];
	let mut z = [0.0];
	match transform.transform_coords(&mut x, &mut y, &mut z) {
		Ok(()) => coord! { x: x[0], y: y[0] },
		Err(_) => coord! { x: f64::NAN, y: f64::NAN },
	}
}

#[cfg(test)]
mod tests {
	use super::{super::projection::Laea, *};

	/// Reference values from PROJ (`cs2cs EPSG:3035 EPSG:4326`), the same ones
	/// the hand-written LAEA is checked against.
	#[test]
	fn gdal_and_the_native_laea_agree() -> Result<()> {
		let gdal = GdalProjection::new(3035)?;
		let native = Laea::etrs89();

		for (easting, northing) in [
			(4_321_000.0, 3_210_000.0),
			(4_341_000.0, 2_691_000.0),
			(4_551_802.0, 3_271_029.0),
		] {
			let source = coord! { x: easting, y: northing };
			let (by_gdal, by_native) = (gdal.to_mercator(source), native.to_mercator(source));
			assert!(
				(by_gdal.x - by_native.x).abs() < 0.02 && (by_gdal.y - by_native.y).abs() < 0.02,
				"{source:?}: gdal {by_gdal:?} vs native {by_native:?}"
			);
		}
		Ok(())
	}

	#[test]
	fn a_national_crs_round_trips() -> Result<()> {
		// EPSG:28992, the Dutch grid statistics CRS, which has no native
		// implementation and is the reason this exists.
		let rd = GdalProjection::new(28992)?;
		let source = coord! { x: 64_300.0, y: 456_700.0 };
		let back = rd.to_source(rd.to_mercator(source));
		assert!(
			(back.x - source.x).abs() < 0.01 && (back.y - source.y).abs() < 0.01,
			"{source:?} came back as {back:?}"
		);
		Ok(())
	}

	#[test]
	#[ignore = "measurement, not a test"]
	fn measure_transform_costs() -> Result<()> {
		use std::time::Instant;
		const N: usize = 100_000;

		let gdal = GdalProjection::new(28992)?;
		let native = Laea::etrs89();
		let points: Vec<Coord<f64>> = (0..N)
			.map(|i| coord! { x: 100_000.0 + (i % 1000) as f64, y: 450_000.0 + (i / 1000) as f64 })
			.collect();

		let t = Instant::now();
		for p in &points {
			std::hint::black_box(gdal.to_mercator(*p));
		}
		let one_by_one = t.elapsed();

		let t = Instant::now();
		let mut xs: Vec<f64> = points.iter().map(|c| c.x).collect();
		let mut ys: Vec<f64> = points.iter().map(|c| c.y).collect();
		let mut zs = vec![0.0; N];
		TRANSFORMS.with_borrow_mut(|cache| {
			let transforms = cache.get(&28992).expect("built above");
			transforms
				.to_mercator
				.transform_coords(&mut xs, &mut ys, &mut zs)
				.unwrap();
		});
		let batched = t.elapsed();

		let laea_points: Vec<Coord<f64>> = (0..N)
			.map(|i| coord! { x: 4_300_000.0 + (i % 1000) as f64, y: 2_700_000.0 + (i / 1000) as f64 })
			.collect();
		let t = Instant::now();
		for p in &laea_points {
			std::hint::black_box(native.to_mercator(*p));
		}
		let native_time = t.elapsed();

		println!("\n{N} coordinates:");
		println!(
			"  gdal, one call each : {one_by_one:?}  ({:.2} µs/coord)",
			one_by_one.as_secs_f64() * 1e6 / N as f64
		);
		println!(
			"  gdal, one batch     : {batched:?}  ({:.3} µs/coord)",
			batched.as_secs_f64() * 1e6 / N as f64
		);
		println!(
			"  native LAEA         : {native_time:?}  ({:.3} µs/coord)",
			native_time.as_secs_f64() * 1e6 / N as f64
		);
		println!(
			"  batching gains      : {:.0}x",
			one_by_one.as_secs_f64() / batched.as_secs_f64()
		);
		Ok(())
	}

	#[test]
	fn an_unusable_code_fails_when_the_grid_is_built() {
		assert!(GdalProjection::new(0).is_err());
	}
}
