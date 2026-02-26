use super::Instance;
use anyhow::{Result, ensure};
use deadpool::managed::{Manager, Object, Pool, RecycleResult};
use gdal::{Dataset, config::set_config_option};
use imageproc::image::{DynamicImage, RgbImage};
use std::{path::Path, sync::Arc};
use versatiles_core::{GeoBBox, WORLD_SIZE, utils::float_to_int};
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

struct GdalManager {
	open_dataset: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static>,
	reuse_limit: u32,
}

impl Manager for GdalManager {
	type Type = Instance;
	type Error = anyhow::Error;

	async fn create(&self) -> Result<Self::Type, Self::Error> {
		use anyhow::Context;
		let open_dataset = self.open_dataset.clone();
		let result = tokio::task::spawn_blocking(move || {
			let ds = (open_dataset)().context("failed to open GDAL dataset via factory")?;
			Ok(Instance::new(ds))
		})
		.await;

		match result {
			Ok(Ok(instance)) => Ok(instance),
			Ok(Err(e)) => Err(e),
			Err(e) => Err(anyhow::anyhow!("spawn_blocking failed: {e}")),
		}
	}

	async fn recycle(&self, obj: &mut Self::Type, _metrics: &deadpool::managed::Metrics) -> RecycleResult<Self::Error> {
		use deadpool::managed::RecycleError;

		if obj.age() > self.reuse_limit {
			return Err(RecycleError::message("instance exceeded reuse limit"));
		}

		obj.cleanup();
		Ok(())
	}
}

#[derive(Clone)]
pub struct DemSource {
	pool: Pool<GdalManager>,
	bbox: GeoBBox,
	pixel_size: f64,
}

unsafe impl Sync for DemSource {}

impl DemSource {
	#[context("Failed to create GDAL DEM dataset from file {:?}", filename)]
	pub async fn new(filename: &Path, reuse_limit: u32, concurrency_limit: usize) -> Result<DemSource> {
		let path = filename.to_path_buf();
		let factory: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> =
			Arc::new(move || Dataset::open(&path).with_context(|| format!("failed to open GDAL dataset: {path:?}")));
		Self::new_with_factory(factory, reuse_limit, concurrency_limit).await
	}

	#[context("Failed to create GDAL DEM dataset via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
	) -> Result<DemSource> {
		set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;

		let dataset = (open_dataset)()?;
		let instance = Instance::new(dataset);
		let bbox = instance.get_bbox()?;
		let pixel_size = instance.get_pixel_size()?;

		let manager = GdalManager {
			open_dataset,
			reuse_limit: reuse_limit.min(1024),
		};

		let pool = Pool::builder(manager)
			.max_size(concurrency_limit.max(1))
			.build()
			.context("failed to build deadpool")?;

		Ok(DemSource { pool, bbox, pixel_size })
	}

	#[context("Failed to get elevation tile ({width}x{height}) for bbox ({bbox:?}) from GDAL DEM dataset")]
	pub async fn get_elevation_tile(
		&self,
		bbox: &GeoBBox,
		width: usize,
		height: usize,
		encoding: DemEncoding,
	) -> Result<Option<DynamicImage>> {
		let instance: Object<GdalManager> = self
			.pool
			.get()
			.await
			.map_err(|e| anyhow::anyhow!("failed to get instance from pool: {e}"))?;

		let dst = instance.reproject_to_float_dataset(width, height, bbox)?;

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
		&self.bbox
	}

	#[context("Failed to compute max zoom level for tile size {tile_size}")]
	pub fn level_max(&self, tile_size: u32) -> Result<u8> {
		ensure!(tile_size > 0, "tile_size must be > 0");

		let initial_res = WORLD_SIZE / f64::from(tile_size);
		let zf = (initial_res / self.pixel_size).log2().ceil();
		let z: i32 = float_to_int(zf).unwrap_or(0);
		Ok(u8::try_from(z.clamp(0, 31))?)
	}
}

impl std::fmt::Debug for DemSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DemSource")
			.field("pool", &"<deadpool::Pool<GdalManager>>")
			.field("bbox", &self.bbox)
			.field("pixel_size", &self.pixel_size)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::super::super::get_spatial_ref;
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
