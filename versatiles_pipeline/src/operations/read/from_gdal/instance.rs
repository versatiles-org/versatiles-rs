use super::get_spatial_ref;
use anyhow::{Result, ensure};
use gdal::{Dataset, spatial_ref::CoordTransform, vector::Geometry};
use std::fmt::Debug;
use versatiles_core::GeoBBox;
use versatiles_derive::context;

#[derive(Debug)]
pub struct Instance {
	dataset: Dataset,
	age: u32,
}

unsafe impl Sync for Instance {}

impl Instance {
	/// Create a new GDAL dataset instance wrapper.
	pub fn new(dataset: Dataset) -> Self {
		Self { dataset, age: 0 }
	}

	/// Get the current age of the GDAL dataset instance.
	pub fn age(&self) -> u32 {
		self.age
	}

	/// Cleanup the GDAL dataset instance, incrementing its age.
	pub fn cleanup(&mut self) {
		self.age = self.age.wrapping_add(1);
		self.dataset.flush_cache().unwrap();
	}

	/// Access the underlying GDAL dataset.
	pub fn dataset(&self) -> &Dataset {
		&self.dataset
	}

	#[context("Failed to compute bounding box for GDAL dataset")]
	pub fn get_bbox(&self) -> Result<GeoBBox> {
		log::trace!("Computing dataset_bbox()");
		let gt = self
			.dataset
			.geo_transform()
			.context("Failed to get geo transform from GDAL dataset")?;

		log::trace!("geo transform: {gt:?}");

		ensure!(gt[2] == 0.0 || gt[4] == 0.0, "GDAL dataset must not be rotated");

		let width = self.dataset.raster_size().0;
		let height = self.dataset.raster_size().1;
		let spatial_ref = self
			.dataset
			.spatial_ref()
			.context("GDAL dataset must have a spatial reference (SRS) defined")?;

		log::trace!("size: {width}x{height}");
		log::trace!("spatial reference: {:?}", &spatial_ref.to_pretty_wkt());

		let coord_transform = CoordTransform::new(&spatial_ref, &get_spatial_ref(4326)?)
			.context("Failed to create coordinate transform to EPSG:4326")?;

		let bounds = coord_transform.transform_bounds(
			&[
				gt[0],
				gt[3],
				gt[0] + gt[1] * width as f64,
				gt[3] + gt[5] * height as f64,
			],
			21,
		)?;

		// Coordinates seem to be flipped in OGREnvelope
		let mut bbox = GeoBBox::new_normalized(bounds[0], bounds[1], bounds[2], bounds[3]);
		bbox.limit_to_mercator();

		log::trace!("bounding box: {bbox:?}");
		Ok(bbox)
	}

	/// Estimate the dataset's native pixel size **in meters/pixel (EPSG:3857)**.
	///
	/// Implementation details:
	/// * Requires an unrotated GeoTransform.
	/// * Samples the center pixel and its right/down neighbors, transforms those
	///   three points to 3857, and takes the max of the two neighbor distances.
	/// * Returns a strictly positive finite value or an error.
	#[context("Failed to compute pixel size for GDAL dataset")]
	pub fn get_pixel_size(&self) -> Result<f64> {
		log::trace!("Computing dataset_pixel_size()");
		let gt = self
			.dataset
			.geo_transform()
			.context("Failed to get geo transform from GDAL dataset")?;

		// We assume no rotation (consistent with `dataset_bbox`).
		ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

		let srs = self
			.dataset
			.spatial_ref()
			.context("GDAL dataset must have a spatial reference (SRS) defined")?;

		// Helper to map pixel (col,row) to georeferenced coordinates
		let pixel_to_size = |col: f64, row: f64| -> Result<f64> {
			let point =
				|x: f64, y: f64| -> (f64, f64, f64) { (gt[0] + x * gt[1] + y * gt[2], gt[3] + x * gt[4] + y * gt[5], 0.0) };

			let mut geom = Geometry::empty(gdal_sys::OGRwkbGeometryType::wkbLineString)?;
			geom.add_point(point(col, row));
			geom.add_point(point(col + 1.0, row));
			geom.add_point(point(col, row + 1.0));
			geom.set_spatial_ref(srs.clone());
			geom.transform_to_inplace(&get_spatial_ref(3857)?)?;

			let mut p = vec![];
			geom.get_points(&mut p);

			let p0 = &p[0];
			let px = &p[1];
			let py = &p[2];
			let ax = (px.0 - p0.0).powi(2) + (px.1 - p0.1).powi(2);
			let ay = (py.0 - p0.0).powi(2) + (py.1 - p0.1).powi(2);

			Ok(ax.min(ay).sqrt())
		};

		let (width, height) = self.dataset.raster_size();
		let mut size_min = f64::MAX;
		for y in [0.1, 0.5, 0.9] {
			for x in [0.1, 0.5, 0.9] {
				let px = (width as f64) * x;
				let py = (height as f64) * y;
				if let Ok(size) = pixel_to_size(px, py)
					&& size > 0.0
					&& size < size_min
				{
					size_min = size;
				}
			}
		}

		log::trace!("pixel_size: {size_min:.6}");
		ensure!(
			size_min.is_finite() && size_min > 0.0,
			"Invalid pixel size in meters computed"
		);
		Ok(size_min)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gdal::DriverManager;
	use rstest::rstest;

	fn mem_dataset(w: usize, h: usize, bands: usize) -> Dataset {
		let driver = DriverManager::get_driver_by_name("MEM").expect("MEM driver");
		driver
			.create_with_band_type::<u8, _>("", w, h, bands)
			.expect("create mem dataset")
	}

	#[test]
	fn age_increments_monotonically() {
		let ds = mem_dataset(1, 1, 1);
		let mut inst = Instance::new(ds);
		assert_eq!(inst.age(), 0);
		inst.cleanup();
		assert_eq!(inst.age(), 1);
		inst.cleanup();
		assert_eq!(inst.age(), 2);
	}

	#[rstest]
	#[case( 3857, [-2e7, -2e7, 2e7, 2e7], [-179.663, -85.022, 179.663, 85.022])]
	#[case( 4326, [-10.0, -20.0, 30.0, 40.0], [-10.0, -20.0, 30.0, 40.0])]
	#[case(25832, [186073.6, 2214294.0, 714984.2, 5542944.0], [4.623, 20.0, 12.0, 50.039])]
	fn test_get_bbox(#[case] epsg: u32, #[case] bbox_in: [f64; 4], #[case] bbox_out: [f64; 4]) -> Result<()> {
		let mut ds = mem_dataset(100, 100, 1);
		ds.set_spatial_ref(&get_spatial_ref(epsg)?)?;
		ds.set_geo_transform(&[
			bbox_in[0],
			(bbox_in[2] - bbox_in[0]) / 100.0,
			0.0,
			bbox_in[3],
			0.0,
			(bbox_in[1] - bbox_in[3]) / 100.0,
		])?;
		let inst = Instance::new(ds);
		let bbox = inst.get_bbox()?;

		if (bbox.x_min - bbox_out[0]).abs() > 1e-3
			|| (bbox.y_min - bbox_out[1]).abs() > 1e-3
			|| (bbox.x_max - bbox_out[2]).abs() > 1e-3
			|| (bbox.y_max - bbox_out[3]).abs() > 1e-3
		{
			panic!(
				"bbox {:?} is not equal to expected {:?}",
				bbox.as_array().map(|v| (v * 1000.0).round() / 1000.0),
				bbox_out
			);
		}
		Ok(())
	}
}
