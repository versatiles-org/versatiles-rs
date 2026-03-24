use super::{BandMapping, BandMappingItem, Cutline, GdalPool, Instance, ResampleAlg};
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
	cutline: Option<&Cutline>,
	nodata: &[f64],
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

	// Create cutline geometry from WKT — must live until after warp completes
	let cutline_geom = cutline.map(Cutline::create_ogr_geometry).transpose()?;

	unsafe {
		use gdal_sys::{
			CPLErr, CPLGetLastErrorMsg, CSLSetNameValue, GDALChunkAndWarpMulti, GDALCreateGenImgProjTransformer2,
			GDALCreateWarpOperation, GDALCreateWarpOptions, GDALDestroyGenImgProjTransformer, GDALDestroyWarpOperation,
			GDALDestroyWarpOptions, GDALGenImgProjTransform, GDALWarpOperationH,
		};

		let options_ptr = GDALCreateWarpOptions();
		(*options_ptr).hSrcDS = h_src_ds;
		(*options_ptr).hDstDS = h_dst_ds;

		(*options_ptr).papszWarpOptions = CSLSetNameValue(
			(*options_ptr).papszWarpOptions,
			c"NUM_THREADS".as_ptr(),
			c"ALL_CPUS".as_ptr(),
		);

		band_mapping.setup_gdal_warp_options(&mut *options_ptr);

		if !nodata.is_empty() {
			(*options_ptr).papszWarpOptions = CSLSetNameValue(
				(*options_ptr).papszWarpOptions,
				c"UNIFIED_SRC_NODATA".as_ptr(),
				c"YES".as_ptr(),
			);
			let nodata_arr = gdal_sys::CPLMalloc(std::mem::size_of_val(nodata)).cast::<f64>();
			for (i, &val) in nodata.iter().enumerate() {
				nodata_arr.add(i).write(val);
			}
			(*options_ptr).padfSrcNoDataReal = nodata_arr;
		}

		(*options_ptr).eResampleAlg = ResampleAlg::default().as_gdal();
		(*options_ptr).dfWarpMemoryLimit = 512.0 * 1024.0 * 1024.0; // 512 MB

		if let Some(ref geom) = cutline_geom {
			(*options_ptr).hCutline = geom.c_geometry();
		}

		(*options_ptr).pTransformerArg = GDALCreateGenImgProjTransformer2(h_src_ds, h_dst_ds, core::ptr::null_mut());
		(*options_ptr).pfnTransformer = Some(GDALGenImgProjTransform);

		let operation: GDALWarpOperationH = GDALCreateWarpOperation(options_ptr);

		#[allow(clippy::cast_possible_truncation)]
		let rv = GDALChunkAndWarpMulti(
			operation,
			0,
			0,
			i32::try_from(width).unwrap(),
			i32::try_from(height).unwrap(),
		);

		GDALDestroyWarpOperation(operation);
		GDALDestroyGenImgProjTransformer((*options_ptr).pTransformerArg);
		GDALDestroyWarpOptions(options_ptr);

		if rv != CPLErr::CE_None {
			anyhow::bail!("{:?}", CPLGetLastErrorMsg());
		}
	}
	// cutline_geom dropped here — GDAL only borrows hCutline during warp

	log::trace!("reproject_image complete");

	Ok(dst_ds)
}

#[derive(Clone)]
pub struct RasterSource {
	pool: GdalPool,
	band_mapping: Arc<BandMapping>,
	cutline: Option<Cutline>,
	/// Per-band nodata values (one per color band), or empty if no nodata.
	nodata: Vec<f64>,
}

unsafe impl Sync for RasterSource {}

impl RasterSource {
	/// Create a `RasterSource` from a file path.
	#[context("Failed to create RasterSource from file {:?}", filename)]
	pub async fn new(
		filename: &Path,
		reuse_limit: u32,
		concurrency_limit: usize,
		cutline_path: Option<&Path>,
		explicit_bands: Option<Vec<usize>>,
		nodata: Option<Vec<f64>>,
		crs_override: Option<u32>,
	) -> Result<RasterSource> {
		let path = filename.to_path_buf();
		let factory: Arc<dyn Fn() -> Result<gdal::Dataset> + Send + Sync + 'static> = Arc::new(move || {
			let mut ds = gdal::Dataset::open(&path).with_context(|| format!("failed to open GDAL dataset: {path:?}"))?;
			if let Some(epsg) = crs_override {
				let srs = super::get_spatial_ref(epsg)?;
				ds.set_spatial_ref(&srs)
					.with_context(|| format!("failed to set CRS override to EPSG:{epsg}"))?;
			}
			Ok(ds)
		});
		Self::new_with_factory(
			factory,
			reuse_limit,
			concurrency_limit,
			cutline_path,
			explicit_bands,
			nodata,
		)
		.await
	}

	/// Create a `RasterSource` from a factory that opens a fresh GDAL `Dataset` on demand.
	#[context("Failed to create RasterSource via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<gdal::Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
		cutline_path: Option<&Path>,
		explicit_bands: Option<Vec<usize>>,
		nodata: Option<Vec<f64>>,
	) -> Result<RasterSource> {
		let (pool, probe) = GdalPool::new_with_factory(open_dataset, reuse_limit, concurrency_limit).await?;

		// Use explicit bands if provided, otherwise auto-detect from color interpretation
		let band_mapping = if let Some(bands) = explicit_bands {
			BandMapping::from_bands(bands)?
		} else {
			BandMapping::try_from(&probe)?
		};
		log::trace!("Band mapping: {band_mapping:?}");

		let n_color_bands = band_mapping.color_band_count();

		// Resolve per-band nodata values:
		// - Explicit per-band values from user → use as-is (must match band count)
		// - Single explicit value → replicate to all color bands
		// - No explicit value → read from source dataset bands
		let effective_nodata: Vec<f64> = if let Some(vals) = nodata {
			if vals.len() == 1 {
				vec![vals[0]; n_color_bands]
			} else {
				ensure!(
					vals.len() == n_color_bands,
					"nodata has {} values but band mapping has {} color bands",
					vals.len(),
					n_color_bands
				);
				vals
			}
		} else {
			// Auto-detect from source bands
			let auto: Vec<f64> = band_mapping
				.iter()
				.filter_map(|item| probe.rasterband(item.band_index).ok().and_then(|b| b.no_data_value()))
				.collect();
			// Only use auto-detected values if every color band has one
			if auto.len() == n_color_bands { auto } else { vec![] }
		};

		let cutline = if let Some(path) = cutline_path {
			let srs = probe
				.spatial_ref()
				.context("GDAL dataset must have a spatial reference for cutline support")?;
			Some(Cutline::from_geojson(path, &srs)?)
		} else {
			None
		};

		Ok(RasterSource {
			pool,
			band_mapping: Arc::new(band_mapping),
			cutline,
			nodata: effective_nodata,
		})
	}

	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	pub async fn get_image(&self, bbox: &GeoBBox, width: usize, height: usize) -> Result<Option<DynamicImage>> {
		let band_mapping = self.band_mapping.clone();
		let cutline = self.cutline.clone();
		let nodata = self.nodata.clone();

		// Get instance from pool - single synchronization point!
		let instance = self.pool.get_instance().await?;

		// Run GDAL reprojection + pixel reading on a blocking thread so we
		// don't block the async executor.  This is the CPU/IO-heavy part.
		let band_mapping2 = self.band_mapping.clone();
		let channel_count = band_mapping2.len(); // color bands + alpha
		let bbox = *bbox;
		let image = tokio::task::spawn_blocking(move || -> Result<Option<DynamicImage>> {
			let dst = reproject_to_dataset(
				&instance,
				width,
				height,
				&bbox,
				&band_mapping,
				cutline.as_ref(),
				&nodata,
			)?;
			// Instance automatically returned to pool when `instance` is dropped
			let band_mapping = band_mapping2;
			let mut buf = vec![0u8; width * height * channel_count];

			// Read color bands
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

			// Read alpha band (always the last band in the destination)
			let alpha_channel_index = band_mapping.color_band_count();
			let alpha_band_index = band_mapping.color_band_count() + 1;
			let band = dst.rasterband(alpha_band_index)?.read_band_as::<u8>()?;
			let data = band.data();
			ensure!(
				data.len() == width * height,
				"Alpha band data length mismatch: expected {} but got {}",
				width * height,
				data.len()
			);
			for (i, &px) in data.iter().enumerate() {
				buf[i * channel_count + alpha_channel_index] = px;
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

	pub fn cutline_bbox(&self) -> Option<&GeoBBox> {
		self.cutline.as_ref().map(Cutline::bbox_wgs84)
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
	use versatiles_image::{DynamicImageTraitOperation, DynamicImageTraitTest, compare_marker_result};

	struct DatasetFactory {
		/// Number of source bands (as the GDAL dataset would have).
		src_band_count: usize,
		geotransform: [f64; 6],
		size: usize,
	}

	impl DatasetFactory {
		pub fn new(bbox: GeoBBox, channel_count: usize) -> DatasetFactory {
			let size = 256;

			let geotransform = [
				bbox.x_min,
				(bbox.x_max - bbox.x_min) / size as f64,
				0.0,
				bbox.y_max,
				0.0,
				(bbox.y_min - bbox.y_max) / size as f64,
			];

			DatasetFactory {
				src_band_count: channel_count,
				geotransform,
				size,
			}
		}

		pub fn get_factory(&self) -> Arc<dyn Fn() -> Result<gdal::Dataset> + Send + Sync + 'static> {
			let src_band_count = self.src_band_count;
			let geotransform_c = self.geotransform;
			let size = self.size;
			Arc::new(move || -> Result<gdal::Dataset> {
				let driver = DriverManager::get_driver_by_name("MEM")?;
				let mut ds = driver.create_with_band_type::<u8, _>("in memory dataset", size, size, src_band_count)?;
				ds.set_spatial_ref(&get_spatial_ref(4326)?)?;
				ds.set_geo_transform(&geotransform_c)?;

				let mut parameters = vec![];
				for band_index in 1..=src_band_count {
					parameters.push(versatiles_image::MarkerParameters {
						offset: 0.0,
						scale: 200.0,
						angle: 80.0 + (band_index as f64 - 1.0) * 90.0,
					});
				}
				let image = DynamicImage::new_marker(&parameters);
				for c in 1..=src_band_count {
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
			futures::executor::block_on(RasterSource::new_with_factory(factory, 1, 2, None, None, None))
		}
	}

	#[rstest]
	// color_channels: number of color channels in the output (excluding alpha)
	#[case(1, ColorType::La8, 1)]
	#[case(2, ColorType::La8, 1)] // 2 Undefined bands → grey + alpha, so 1 color channel
	#[case(3, ColorType::Rgba8, 3)]
	#[case(4, ColorType::Rgba8, 3)] // 4 bands → RGB + alpha, so 3 color channels
	#[tokio::test(flavor = "multi_thread")]
	async fn test_dataset_get_image2(
		#[case] channels: usize,
		#[case] expected_color: ColorType,
		#[case] color_channels: usize,
	) {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = RasterSource::from_testdata(bbox_in, channels).unwrap();
		let image = ds.get_image(&bbox_in, 256, 256).await.unwrap().unwrap();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), expected_color);
		// Strip alpha before gauging marker pattern (alpha is always added by BandMapping)
		let image_no_alpha = image.as_no_alpha().unwrap();
		let results = image_no_alpha.gauge_marker();

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
		compare_marker_result(&expected_results[0..color_channels], &results).unwrap();
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
