use super::{BandMapping, BandMappingItem, GdalPool, Instance, ResampleAlg};
use anyhow::{Result, ensure};
use gdal::{Dataset, GeoTransform};
use imageproc::image::DynamicImage;
use std::{path::Path, sync::Arc};
use versatiles_core::GeoBBox;
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitConvert;

/// Reproject the source dataset into an 8-bit in-memory dataset in EPSG:3857.
fn reproject_to_dataset(
	instance: &Instance,
	width: usize,
	height: usize,
	bbox: &GeoBBox,
	band_mapping: &Arc<BandMapping>,
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

	let h_src_ds = instance.dataset().c_dataset();
	let h_dst_ds = dst_ds.c_dataset();

	unsafe {
		use gdal_sys::{
			CPLErr, CPLGetLastErrorMsg, CSLSetNameValue, GDALChunkAndWarpMulti, GDALCreateGenImgProjTransformer2,
			GDALCreateWarpOperation, GDALCreateWarpOptions, GDALDestroyGenImgProjTransformer, GDALDestroyWarpOperation,
			GDALGenImgProjTransform, GDALWarpOperationH, GDALWarpOptions,
		};

		let mut options: GDALWarpOptions = *GDALCreateWarpOptions();
		options.hSrcDS = h_src_ds;
		options.hDstDS = h_dst_ds;

		CSLSetNameValue(options.papszWarpOptions, c"NUM_THREADS".as_ptr(), c"ALL_CPUS".as_ptr());

		band_mapping.setup_gdal_warp_options(&mut options);

		options.eResampleAlg = ResampleAlg::default().as_gdal();
		options.dfWarpMemoryLimit = 512.0 * 1024.0 * 1024.0; // 512 MB

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
			anyhow::bail!("{:?}", CPLGetLastErrorMsg());
		}
	}

	log::trace!("reproject_image complete");

	Ok(dst_ds)
}

#[derive(Clone)]
pub struct RasterSource {
	pool: GdalPool,
	band_mapping: Arc<BandMapping>,
}

unsafe impl Sync for RasterSource {}

impl RasterSource {
	/// Create a `RasterSource` from a file path.
	#[context("Failed to create RasterSource from file {:?}", filename)]
	pub async fn new(filename: &Path, reuse_limit: u32, concurrency_limit: usize) -> Result<RasterSource> {
		let pool = GdalPool::new(filename, reuse_limit, concurrency_limit).await?;

		// Probe band mapping from one instance
		let instance = pool.get_instance().await?;
		let band_mapping = BandMapping::try_from(instance.dataset())?;
		log::trace!("Band mapping: {band_mapping:?}");

		Ok(RasterSource {
			pool,
			band_mapping: Arc::new(band_mapping),
		})
	}

	/// Create a `RasterSource` from a factory that opens a fresh GDAL `Dataset` on demand.
	#[cfg(test)]
	#[context("Failed to create RasterSource via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<gdal::Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
	) -> Result<RasterSource> {
		let pool = GdalPool::new_with_factory(open_dataset, reuse_limit, concurrency_limit).await?;

		// Probe band mapping from one instance
		let instance = pool.get_instance().await?;
		let band_mapping = BandMapping::try_from(instance.dataset())?;
		log::trace!("Band mapping: {band_mapping:?}");

		Ok(RasterSource {
			pool,
			band_mapping: Arc::new(band_mapping),
		})
	}

	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	pub async fn get_image(&self, bbox: &GeoBBox, width: usize, height: usize) -> Result<Option<DynamicImage>> {
		let band_mapping = self.band_mapping.clone();

		// Get instance from pool - single synchronization point!
		let instance = self.pool.get_instance().await?;
		let dst = reproject_to_dataset(&instance, width, height, bbox, &band_mapping)?;
		// Instance automatically returned to pool when dropped

		let band_mapping = self.band_mapping.clone();
		let channel_count = band_mapping.len();
		let image = tokio::task::spawn_blocking(move || -> Result<Option<DynamicImage>> {
			let mut buf = vec![0u8; width * height * channel_count];
			for BandMappingItem {
				channel_index,
				band_index,
			} in band_mapping.iter()
			{
				let band = dst.rasterband(band_index)?.read_band_as::<u8>()?;
				let data = band.data();
				ensure!(
					data.len() == width * height,
					"Band {} data length mismatch: expected {} but got {}",
					band_index,
					width * height,
					data.len()
				);
				for (i, &px) in data.iter().enumerate() {
					buf[i * channel_count + channel_index] = px;
				}
			}
			log::trace!("Filled image buffer ({} bytes)", buf.len());
			let img =
				DynamicImage::from_raw(width, height, buf).context("Failed to create DynamicImage from GDAL dataset")?;
			Ok(Some(img))
		})
		.await??;

		Ok(image)
	}

	pub fn bbox(&self) -> &GeoBBox {
		self.pool.bbox()
	}

	pub fn level_max(&self, tile_size: u32) -> Result<u8> {
		self.pool.level_max(tile_size)
	}
}

impl std::fmt::Debug for RasterSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RasterSource")
			.field("pool", &self.pool)
			.field("band_mapping", &self.band_mapping)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::super::get_spatial_ref;
	use super::*;
	use gdal::DriverManager;
	use imageproc::image::ColorType;
	use rstest::rstest;
	use std::vec;
	use versatiles_image::{DynamicImageTraitTest, compare_marker_result};

	struct DatasetFactory {
		band_mapping: BandMapping,
		geotransform: [f64; 6],
		size: usize,
	}

	impl DatasetFactory {
		pub fn new(bbox: GeoBBox, channel_count: usize) -> DatasetFactory {
			let size = 256;
			let band_mapping = BandMapping::new((0..channel_count).map(|i| i + 1).collect());

			let geotransform = [
				bbox.x_min,
				(bbox.x_max - bbox.x_min) / size as f64,
				0.0,
				bbox.y_max,
				0.0,
				(bbox.y_min - bbox.y_max) / size as f64,
			];

			DatasetFactory {
				band_mapping,
				geotransform,
				size,
			}
		}

		pub fn get_factory(&self) -> Arc<dyn Fn() -> Result<gdal::Dataset> + Send + Sync + 'static> {
			let band_mapping_c = self.band_mapping.clone();
			let geotransform_c = self.geotransform;
			let size = self.size;
			Arc::new(move || -> Result<gdal::Dataset> {
				let driver = DriverManager::get_driver_by_name("MEM")?;
				let mut ds =
					driver.create_with_band_type::<u8, _>("in memory dataset", size, size, band_mapping_c.len())?;
				ds.set_spatial_ref(&get_spatial_ref(4326)?)?;
				ds.set_geo_transform(&geotransform_c)?;

				let mut parameters = vec![];
				for band_index in 1..=band_mapping_c.len() {
					parameters.push(versatiles_image::MarkerParameters {
						offset: 0.0,
						scale: 200.0,
						angle: 80.0 + (band_index as f64 - 1.0) * 90.0,
					});
				}
				let image = DynamicImage::new_marker(&parameters);
				for c in 1..=band_mapping_c.len() {
					let data = image.iter_pixels().map(|p| p[c - 1]).collect();
					let mut buffer = gdal::raster::Buffer::new((size, size), data);
					ds.rasterband(c)?.write((0, 0), (size, size), &mut buffer)?;
				}
				Ok(ds)
			})
		}
	}

	impl RasterSource {
		pub fn from_testdata(bbox: GeoBBox, channel_count: usize) -> Result<RasterSource> {
			let factory = DatasetFactory::new(bbox, channel_count).get_factory();
			// Construct via the factory (seed one instance inside new_with_factory)
			futures::executor::block_on(RasterSource::new_with_factory(factory, 1, 2))
		}
	}

	#[rstest]
	#[case(1, ColorType::L8)]
	#[case(2, ColorType::La8)]
	#[case(3, ColorType::Rgb8)]
	#[case(4, ColorType::Rgba8)]
	#[tokio::test(flavor = "multi_thread")]
	async fn test_dataset_get_image2(#[case] channels: usize, #[case] expected_color: ColorType) {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = RasterSource::from_testdata(bbox_in, channels).unwrap();
		let image = ds.get_image(&bbox_in, 256, 256).await.unwrap().unwrap();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), expected_color);
		let results = image.gauge_marker();

		let expected_results = [
			(2.8, 200.0, 80.0),
			(0.5, 200.0, 170.0),
			(-2.8, 200.0, 260.0),
			(-0.5, 200.0, 350.0),
		]
		.map(|p| versatiles_image::MarkerParameters {
			offset: p.0,
			scale: p.1,
			angle: p.2,
		});
		compare_marker_result(&expected_results[0..channels], &results).unwrap();
	}

	#[test]
	fn test_bbox() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = RasterSource::from_testdata(bbox_in, 3).unwrap();
		let bbox_out = ds.bbox();
		assert!((bbox_out.x_min - 14.0).abs() < 0.001);
		assert!((bbox_out.y_min - 49.0).abs() < 0.001);
		assert!((bbox_out.x_max - 24.0).abs() < 0.001);
		assert!((bbox_out.y_max - 55.0).abs() < 0.001);
	}

	#[test]
	fn test_level_max() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = RasterSource::from_testdata(bbox_in, 3).unwrap();
		// Should return a reasonable zoom level
		let level = ds.level_max(256).unwrap();
		assert!(level > 0);
		assert!(level <= 31);
		// Smaller tile size should give higher max zoom
		let level_512 = ds.level_max(512).unwrap();
		assert!(level_512 <= level);
	}

	#[test]
	fn test_level_max_zero_tile_size_fails() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = RasterSource::from_testdata(bbox_in, 3).unwrap();
		let result = ds.level_max(0);
		assert!(result.is_err());
	}

	#[test]
	fn test_debug() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = RasterSource::from_testdata(bbox_in, 3).unwrap();
		let debug_str = format!("{ds:?}");
		assert!(debug_str.contains("RasterSource"));
		assert!(debug_str.contains("bbox"));
		assert!(debug_str.contains("band_mapping"));
		assert!(debug_str.contains("pixel_size"));
	}
}
