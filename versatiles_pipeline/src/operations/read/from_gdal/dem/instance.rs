use super::super::{ResampleAlg, get_spatial_ref};
use anyhow::{Context, Result, bail, ensure};
use gdal::{Dataset, DriverManager, GeoTransform, spatial_ref::CoordTransform, vector::Geometry};
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
	pub fn new(dataset: Dataset) -> Self {
		Self { dataset, age: 0 }
	}

	pub fn age(&self) -> u32 {
		self.age
	}

	pub fn cleanup(&mut self) {
		self.age = self.age.wrapping_add(1);
		self.dataset.flush_cache().unwrap();
	}

	/// Reproject the source dataset into a 1-band Float32 in-memory dataset
	/// covering the given bbox in Web Mercator (EPSG:3857).
	pub fn reproject_to_float_dataset(&self, width: usize, height: usize, bbox: &GeoBBox) -> Result<Dataset> {
		log::trace!("reproject_to_float_dataset started for size={width}x{height}");

		let bbox_mer = bbox.to_mercator();

		// Create 1-band Float32 MEM dataset
		let driver = DriverManager::get_driver_by_name("MEM").context("Failed to get GDAL MEM driver")?;
		let mut dst_ds = driver
			.create_with_band_type::<f32, _>("mem", width, height, 1)
			.context("Failed to create Float32 in-memory dataset")?;
		dst_ds.set_spatial_ref(&get_spatial_ref(3857)?)?;

		let geo_transform: GeoTransform = [
			bbox_mer[0],
			(bbox_mer[2] - bbox_mer[0]) / width as f64,
			0.0,
			bbox_mer[3],
			0.0,
			(bbox_mer[1] - bbox_mer[3]) / height as f64,
		];
		dst_ds.set_geo_transform(&geo_transform)?;

		let h_src_ds = self.dataset.c_dataset();
		let h_dst_ds = dst_ds.c_dataset();

		unsafe {
			use gdal_sys::{
				CPLErr, CPLGetLastErrorMsg, CPLMalloc, CSLSetNameValue, GDALChunkAndWarpMulti,
				GDALCreateGenImgProjTransformer2, GDALCreateWarpOperation, GDALCreateWarpOptions,
				GDALDestroyGenImgProjTransformer, GDALDestroyWarpOperation, GDALGenImgProjTransform, GDALWarpOperationH,
				GDALWarpOptions,
			};

			let mut options: GDALWarpOptions = *GDALCreateWarpOptions();
			options.hSrcDS = h_src_ds;
			options.hDstDS = h_dst_ds;

			CSLSetNameValue(options.papszWarpOptions, c"NUM_THREADS".as_ptr(), c"ALL_CPUS".as_ptr());

			// Band mapping: source band 1 → dest band 1
			options.nBandCount = 1;
			let n = std::mem::size_of::<i32>();
			options.panSrcBands = CPLMalloc(n).cast::<i32>();
			options.panDstBands = CPLMalloc(n).cast::<i32>();
			options.panSrcBands.write(1);
			options.panDstBands.write(1);

			// Use Bilinear for DEM — preserves elevation values better than averaging
			options.eResampleAlg = ResampleAlg::Bilinear.as_gdal();
			options.dfWarpMemoryLimit = 512.0 * 1024.0 * 1024.0;

			options.pTransformerArg = GDALCreateGenImgProjTransformer2(h_src_ds, h_dst_ds, core::ptr::null_mut());
			options.pfnTransformer = Some(GDALGenImgProjTransform);

			let operation: GDALWarpOperationH = GDALCreateWarpOperation(&raw const options);

			#[allow(clippy::cast_possible_truncation)]
			let rv = GDALChunkAndWarpMulti(
				operation,
				0,
				0,
				i32::try_from(width).unwrap(),
				i32::try_from(height).unwrap(),
			);

			GDALDestroyWarpOperation(operation);
			GDALDestroyGenImgProjTransformer(options.pTransformerArg);

			if rv != CPLErr::CE_None {
				bail!("{:?}", CPLGetLastErrorMsg());
			}
		}

		log::trace!("reproject_to_float_dataset complete");
		Ok(dst_ds)
	}

	#[context("Failed to compute bounding box for GDAL dataset")]
	pub fn get_bbox(&self) -> Result<GeoBBox> {
		log::trace!("Computing dataset_bbox()");
		let gt = self
			.dataset
			.geo_transform()
			.context("Failed to get geo transform from GDAL dataset")?;

		ensure!(gt[2] == 0.0 || gt[4] == 0.0, "GDAL dataset must not be rotated");

		let width = self.dataset.raster_size().0;
		let height = self.dataset.raster_size().1;
		let spatial_ref = self
			.dataset
			.spatial_ref()
			.context("GDAL dataset must have a spatial reference (SRS) defined")?;

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

		let mut bbox = GeoBBox::new_normalized(bounds[0], bounds[1], bounds[2], bounds[3]);
		bbox.limit_to_mercator();

		Ok(bbox)
	}

	#[context("Failed to compute pixel size for GDAL dataset")]
	pub fn get_pixel_size(&self) -> Result<f64> {
		log::trace!("Computing dataset_pixel_size()");
		let gt = self
			.dataset
			.geo_transform()
			.context("Failed to get geo transform from GDAL dataset")?;

		ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

		let srs = self
			.dataset
			.spatial_ref()
			.context("GDAL dataset must have a spatial reference (SRS) defined")?;

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

	fn mem_float_dataset(w: usize, h: usize) -> Dataset {
		let driver = DriverManager::get_driver_by_name("MEM").expect("MEM driver");
		driver
			.create_with_band_type::<f32, _>("", w, h, 1)
			.expect("create mem dataset")
	}

	#[test]
	fn age_increments_monotonically() {
		let ds = mem_float_dataset(1, 1);
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
		let mut ds = mem_float_dataset(100, 100);
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
