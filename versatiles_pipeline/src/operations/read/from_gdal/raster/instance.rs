use super::{BandMapping, ResampleAlg};
use anyhow::{Result, bail};
use gdal::{Dataset, GeoTransform};
use gdal_sys::{CPLErr, CPLGetLastErrorMsg, GDALReprojectImage};
use std::{
	ptr::{null, null_mut},
	sync::Arc,
};
use versatiles_core::GeoBBox;

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

	pub fn reproject_to_dataset(
		&self,
		width: u32,
		height: u32,
		bbox: &GeoBBox,
		band_mapping: Arc<BandMapping>,
	) -> Result<Dataset> {
		log::trace!("reproject_image started for size={width}x{height}");

		let bbox_arr = bbox.wgs84_as_mercator();
		let src_h = self.dataset.c_dataset();
		let mut dst_ds = band_mapping.create_mem_dataset(width, height)?;
		let geo_transform: GeoTransform = [
			bbox_arr[0],
			(bbox_arr[2] - bbox_arr[0]) / width as f64,
			0.0,
			bbox_arr[3],
			0.0,
			(bbox_arr[1] - bbox_arr[3]) / height as f64,
		];
		dst_ds.set_geo_transform(&geo_transform)?;

		unsafe {
			let rv = GDALReprojectImage(
				src_h,
				null(),
				dst_ds.c_dataset(),
				null(),
				ResampleAlg::default().as_gdal(),
				0.0,
				0.0,
				None,
				null_mut(),
				null_mut(),
			);

			if rv != CPLErr::CE_None {
				bail!("{:?}", CPLGetLastErrorMsg());
			}
		};

		log::trace!("reproject_image complete");

		Ok(dst_ds)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gdal::DriverManager;

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
}
