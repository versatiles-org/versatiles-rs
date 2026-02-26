use super::{GdalPool, Instance, ResampleAlg, get_spatial_ref};
use anyhow::{Context, Result, bail, ensure};
use gdal::{Dataset, DriverManager, GeoTransform};
use imageproc::image::{DynamicImage, RgbImage};
use std::path::Path;
#[cfg(test)]
use std::sync::Arc;
use versatiles_core::GeoBBox;
use versatiles_derive::context;

/// DEM encoding format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemEncoding {
	Mapbox,
	Terrarium,
}

/// Encode an elevation value (in meters) into an RGB triplet.
///
/// **Mapbox Terrain-RGB**: `raw = round((elevation + 10000) / 0.1)`
/// **Terrarium**: `raw = round((elevation + 32768) * 256)`
///
/// Both clamp to `[0, 0x00FFFFFF]` (24-bit range).
pub fn encode_elevation(elevation: f32, encoding: DemEncoding) -> [u8; 3] {
	let raw = match encoding {
		DemEncoding::Mapbox => ((f64::from(elevation) + 10000.0) * 10.0).round(),
		DemEncoding::Terrarium => ((f64::from(elevation) + 32768.0) * 256.0).round(),
	};
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	let raw = (raw as i64).clamp(0, 0x00FF_FFFF) as u32;
	[
		((raw >> 16) & 0xFF) as u8,
		((raw >> 8) & 0xFF) as u8,
		(raw & 0xFF) as u8,
	]
}

/// Reproject the source dataset into a 1-band Float32 in-memory dataset
/// covering the given bbox in Web Mercator (EPSG:3857).
fn reproject_to_float_dataset(instance: &Instance, width: usize, height: usize, bbox: &GeoBBox) -> Result<Dataset> {
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

	let h_src_ds = instance.dataset().c_dataset();
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

		// Band mapping: source band 1 -> dest band 1
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

#[derive(Clone)]
pub struct DemSource {
	pool: GdalPool,
}

unsafe impl Sync for DemSource {}

impl DemSource {
	#[context("Failed to create DemSource from file {:?}", filename)]
	pub async fn new(filename: &Path, reuse_limit: u32, concurrency_limit: usize) -> Result<DemSource> {
		let pool = GdalPool::new(filename, reuse_limit, concurrency_limit).await?;
		Ok(DemSource { pool })
	}

	#[cfg(test)]
	#[context("Failed to create DemSource via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
	) -> Result<DemSource> {
		let pool = GdalPool::new_with_factory(open_dataset, reuse_limit, concurrency_limit).await?;
		Ok(DemSource { pool })
	}

	#[context("Failed to get elevation tile ({width}x{height}) for bbox ({bbox:?}) from GDAL DEM dataset")]
	pub async fn get_elevation_tile(
		&self,
		bbox: &GeoBBox,
		width: usize,
		height: usize,
		encoding: DemEncoding,
	) -> Result<Option<DynamicImage>> {
		let instance = self.pool.get_instance().await?;

		let dst = reproject_to_float_dataset(&instance, width, height, bbox)?;

		let image = tokio::task::spawn_blocking(move || -> Result<Option<DynamicImage>> {
			let band = dst.rasterband(1)?.read_band_as::<f32>()?;
			let data = band.data();
			ensure!(
				data.len() == width * height,
				"Band data length mismatch: expected {} but got {}",
				width * height,
				data.len()
			);

			let mut rgb_buf = vec![0u8; width * height * 3];
			for (i, &elev) in data.iter().enumerate() {
				let [r, g, b] = encode_elevation(elev, encoding);
				rgb_buf[i * 3] = r;
				rgb_buf[i * 3 + 1] = g;
				rgb_buf[i * 3 + 2] = b;
			}

			#[allow(clippy::cast_possible_truncation)]
			let img = RgbImage::from_raw(width as u32, height as u32, rgb_buf)
				.context("Failed to create RgbImage from elevation data")?;
			Ok(Some(DynamicImage::ImageRgb8(img)))
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

impl std::fmt::Debug for DemSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DemSource").field("pool", &self.pool).finish()
	}
}

#[cfg(test)]
mod tests {
	use super::super::get_spatial_ref;
	use super::*;
	use gdal::DriverManager;
	use rstest::rstest;

	struct DemDatasetFactory {
		geotransform: [f64; 6],
		size: usize,
	}

	impl DemDatasetFactory {
		pub fn new(bbox: GeoBBox) -> Self {
			let size = 256;
			let geotransform = [
				bbox.x_min,
				(bbox.x_max - bbox.x_min) / size as f64,
				0.0,
				bbox.y_max,
				0.0,
				(bbox.y_min - bbox.y_max) / size as f64,
			];
			DemDatasetFactory { geotransform, size }
		}

		pub fn get_factory(&self) -> Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> {
			let geotransform_c = self.geotransform;
			let size = self.size;
			Arc::new(move || -> Result<Dataset> {
				let driver = DriverManager::get_driver_by_name("MEM")?;
				let mut ds = driver.create_with_band_type::<f32, _>("in memory dem", size, size, 1)?;
				ds.set_spatial_ref(&get_spatial_ref(4326)?)?;
				ds.set_geo_transform(&geotransform_c)?;

				// Fill with a gradient elevation: 0..8848 meters
				let mut elev_data = vec![0.0f32; size * size];
				for row in 0..size {
					for col in 0..size {
						elev_data[row * size + col] = (col as f32 / size as f32) * 8848.0;
					}
				}
				let mut buffer = gdal::raster::Buffer::new((size, size), elev_data);
				ds.rasterband(1)?.write((0, 0), (size, size), &mut buffer)?;
				Ok(ds)
			})
		}
	}

	impl DemSource {
		pub fn from_testdata(bbox: GeoBBox) -> Result<DemSource> {
			let factory = DemDatasetFactory::new(bbox).get_factory();
			futures::executor::block_on(DemSource::new_with_factory(factory, 1, 2))
		}
	}

	#[test]
	fn test_encode_elevation_mapbox() {
		// 0m: raw = (0 + 10000) / 0.1 = 100000
		let [r, g, b] = encode_elevation(0.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 100_000);

		// 100m: raw = (100 + 10000) / 0.1 = 101000
		let [r, g, b] = encode_elevation(100.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 101_000);

		// -100m: raw = (-100 + 10000) / 0.1 = 99000
		let [r, g, b] = encode_elevation(-100.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 99_000);

		// 8848m (Everest): raw = (8848 + 10000) / 0.1 = 188480
		let [r, g, b] = encode_elevation(8848.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 188_480);
	}

	#[test]
	fn test_encode_elevation_terrarium() {
		// 0m: raw = (0 + 32768) * 256 = 8388608
		let [r, g, b] = encode_elevation(0.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 8_388_608);

		// 100m: raw = (100 + 32768) * 256 = 8414208
		let [r, g, b] = encode_elevation(100.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 8_414_208);

		// -100m: raw = (-100 + 32768) * 256 = 8363008
		let [r, g, b] = encode_elevation(-100.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 8_363_008);

		// 8848m: raw = (8848 + 32768) * 256 = 10653696
		let [r, g, b] = encode_elevation(8848.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 10_653_696);
	}

	#[test]
	fn test_encode_elevation_clamping() {
		// Very negative: should clamp to 0
		let [r, g, b] = encode_elevation(-20000.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 0);

		// Very high: should clamp to 0x00FFFFFF
		let [r, g, b] = encode_elevation(2_000_000.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 0x00FF_FFFF);
	}

	#[test]
	fn test_bbox() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let bbox_out = ds.bbox();
		assert!((bbox_out.x_min - 14.0).abs() < 0.001);
		assert!((bbox_out.y_min - 49.0).abs() < 0.001);
		assert!((bbox_out.x_max - 24.0).abs() < 0.001);
		assert!((bbox_out.y_max - 55.0).abs() < 0.001);
	}

	#[test]
	fn test_level_max() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let level = ds.level_max(256).unwrap();
		assert!(level > 0);
		assert!(level <= 31);
		let level_512 = ds.level_max(512).unwrap();
		assert!(level_512 <= level);
	}

	#[test]
	fn test_level_max_zero_tile_size_fails() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let result = ds.level_max(0);
		assert!(result.is_err());
	}

	#[rstest]
	#[case(DemEncoding::Mapbox)]
	#[case(DemEncoding::Terrarium)]
	#[tokio::test(flavor = "multi_thread")]
	async fn test_get_elevation_tile(#[case] encoding: DemEncoding) {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let image = ds
			.get_elevation_tile(&bbox_in, 256, 256, encoding)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
	}

	#[test]
	fn test_debug() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let debug_str = format!("{ds:?}");
		assert!(debug_str.contains("DemSource"));
		assert!(debug_str.contains("bbox"));
		assert!(debug_str.contains("pixel_size"));
	}
}
