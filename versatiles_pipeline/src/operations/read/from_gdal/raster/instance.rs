use super::{BandMapping, ResampleAlg};
use anyhow::{Result, bail};
use gdal::{Dataset, GeoTransform};
use std::{fmt::Debug, sync::Arc};
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
		width: usize,
		height: usize,
		bbox: &GeoBBox,
		band_mapping: Arc<BandMapping>,
	) -> Result<Dataset> {
		log::trace!("reproject_image started for size={width}x{height}");

		let bbox_mer = bbox.to_mercator();

		let mut dst_ds = band_mapping.create_mem_dataset(width, height)?;
		// GDAL GeoTransform: [origin_x, pixel_width, rot_x, origin_y, rot_y, pixel_height]
		// For north-up images: rot_x = 0, rot_y = 0, pixel_height is **negative**.
		let geo_transform: GeoTransform = [
			bbox_mer[0],                                 // origin_x = minx
			(bbox_mer[2] - bbox_mer[0]) / width as f64,  // pixel width in meters/pixel
			0.0,                                         // rot_x
			bbox_mer[3],                                 // origin_y = maxy (top-left Y)
			0.0,                                         // rot_y
			(bbox_mer[1] - bbox_mer[3]) / height as f64, // pixel height (negative for north-up)
		];
		dst_ds.set_geo_transform(&geo_transform)?;

		let h_src_ds = self.dataset.c_dataset();
		let h_dst_ds = dst_ds.c_dataset();

		unsafe {
			use gdal_sys::*;

			let mut options: GDALWarpOptions = *GDALCreateWarpOptions();
			options.hSrcDS = h_src_ds;
			options.hDstDS = h_dst_ds;

			CSLSetNameValue(
				options.papszWarpOptions,
				b"NUM_THREADS\0".as_ptr() as *const i8,
				b"ALL_CPUS\0".as_ptr() as *const i8,
			);

			band_mapping.setup_gdal_warp_options(&mut options);

			options.eResampleAlg = ResampleAlg::default().as_gdal();
			options.dfWarpMemoryLimit = 512.0 * 1024.0 * 1024.0; // 512 MB

			options.pTransformerArg = GDALCreateGenImgProjTransformer2(h_src_ds, h_dst_ds, core::ptr::null_mut());
			options.pfnTransformer = Some(GDALGenImgProjTransform);

			let operation: GDALWarpOperationH = GDALCreateWarpOperation(&mut options);

			let rv = GDALChunkAndWarpMulti(operation, 0, 0, width as i32, height as i32);

			GDALDestroyWarpOperation(operation);
			GDALDestroyGenImgProjTransformer(options.pTransformerArg);

			if rv != CPLErr::CE_None {
				bail!("{:?}", CPLGetLastErrorMsg());
			}
		}

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
