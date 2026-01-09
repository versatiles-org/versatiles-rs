use super::{BandMapping, BandMappingItem, Instance};
use anyhow::{Result, ensure};
use deadpool::managed::{Manager, Object, Pool, RecycleResult};
use gdal::{Dataset, config::set_config_option};
use imageproc::image::DynamicImage;
use std::{path::Path, sync::Arc};
use versatiles_core::GeoBBox;
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitConvert;

/// Web‑Mercator world circumference in meters (2πR, where R = 6,378,137 m).
/// Used to compute the ground resolution at zoom 0 for a given tile size.
const EARTH_CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * 6_378_137.0;

/// Manager for deadpool that creates and recycles GDAL dataset instances
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
			Err(e) => Err(anyhow::anyhow!("spawn_blocking failed: {}", e)),
		}
	}

	async fn recycle(&self, obj: &mut Self::Type, _metrics: &deadpool::managed::Metrics) -> RecycleResult<Self::Error> {
		use deadpool::managed::RecycleError;

		// Check if instance has exceeded reuse limit
		if obj.age() > self.reuse_limit {
			return Err(RecycleError::message("instance exceeded reuse limit"));
		}

		// Cleanup the instance for reuse
		obj.cleanup();
		Ok(())
	}
}

#[derive(Clone)]
pub struct RasterSource {
	pool: Pool<GdalManager>,
	bbox: GeoBBox,
	band_mapping: Arc<BandMapping>,
	pixel_size: f64,
}

unsafe impl Sync for RasterSource {}

impl RasterSource {
	/// Create a `RasterSource` from a file path.
	#[context("Failed to create GDAL dataset from file {:?}", filename)]
	pub async fn new(filename: &Path, reuse_limit: u32, concurrency_limit: usize) -> Result<RasterSource> {
		let path = filename.to_path_buf();
		let factory: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> =
			Arc::new(move || Dataset::open(&path).with_context(|| format!("failed to open GDAL dataset: {:?}", path)));
		Self::new_with_factory(factory, reuse_limit, concurrency_limit).await
	}

	/// Create a `RasterSource` from a factory that opens a fresh GDAL `Dataset` on demand.
	///
	/// The factory is called whenever the internal pool needs to create a new instance.
	/// This makes it easy to support custom sources (in‑memory MEM, VSICURL, cloud drivers, etc.).
	#[context("Failed to create GDAL dataset via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
	) -> Result<RasterSource> {
		set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;
		log::trace!("GDAL_NUM_THREADS set to ALL_CPUS");

		// Open one dataset to probe metadata
		let dataset = (open_dataset)()?;
		log::trace!(
			"Opened GDAL dataset ({}x{}, bands={})",
			dataset.raster_size().0,
			dataset.raster_size().1,
			dataset.raster_count()
		);
		let instance = Instance::new(dataset);
		let bbox = instance.get_bbox()?;
		let band_mapping = instance.get_band_mapping()?;
		let pixel_size = instance.get_pixel_size()?;
		log::trace!("Dataset pixel_size (m/px): {:.6}", pixel_size);
		log::trace!("Dataset bbox (EPSG:4326): {:?}", bbox);
		log::trace!("Band mapping: {band_mapping:?}");

		// Create deadpool manager and pool - single synchronization point!
		let manager = GdalManager {
			open_dataset,
			reuse_limit: reuse_limit.min(1024),
		};

		let pool = Pool::builder(manager)
			.max_size(concurrency_limit.max(1))
			.build()
			.context("failed to build deadpool")?;

		Ok(RasterSource {
			pool,
			bbox,
			band_mapping: Arc::new(band_mapping),
			pixel_size,
		})
	}

	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	pub async fn get_image(&self, bbox: &GeoBBox, width: usize, height: usize) -> Result<Option<DynamicImage>> {
		let band_mapping = self.band_mapping.clone();

		// Get instance from pool - single synchronization point!
		let instance: Object<GdalManager> = self
			.pool
			.get()
			.await
			.map_err(|e| anyhow::anyhow!("failed to get instance from pool: {}", e))?;
		let dst = instance.reproject_to_dataset(width, height, bbox, band_mapping)?;
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
		&self.bbox
	}

	/// Compute the **maximum** Web‑Mercator zoom level supported by this dataset’s
	/// native ground resolution.
	///
	/// ## How it’s computed
	/// 1. `dataset_pixel_size()` (called during construction) estimates the dataset’s
	///    native resolution at the image center, **in EPSG:3857 meters per pixel**.
	/// 2. For a given `tile_size` (e.g. 256 or 512), the ground resolution at zoom 0 is
	///    `initial_res = (2π · 6_378_137) / tile_size`.
	/// 3. The maximum zoom is:
	///
	///    ```text
	///    z_max = ceil( log2( initial_res / pixel_size_m ) )
	///    ```
	///
	///    This returns the smallest integer zoom whose nominal tile resolution is
	///    **not finer** than the dataset’s native resolution. (gdal2tiles uses a
	///    slightly different guard; if you prefer the exact historical behavior,
	///    use `floor` instead of `ceil`.)
	///
	/// The result is clamped to the range `[0, 31]`.
	///
	/// ## Parameters
	/// * `tile_size` – Tile edge length in pixels (usually 256 or 512). Must be > 0.
	///
	/// ## Returns
	/// * A `u8` max zoom suitable for Web‑Mercator tiling.
	///
	/// ## Pan‑projection note
	/// The dataset can be in any source SRS; its pixel size was measured after
	/// transforming to EPSG:3857, so the returned zoom level is always in the
	/// Web‑Mercator pyramid.
	#[context("Failed to compute max zoom level for tile size {tile_size}")]
	pub fn level_max(&self, tile_size: u32) -> Result<u8> {
		ensure!(tile_size > 0, "tile_size must be > 0");
		log::trace!(
			"level_max(tile_size={}) with pixel_size={:.6}",
			tile_size,
			self.pixel_size
		);

		// Initial resolution (meters per pixel at zoom 0)
		let initial_res = EARTH_CIRCUMFERENCE / (tile_size as f64);
		let zf = (initial_res / self.pixel_size).log2().ceil();
		log::trace!("initial_res={:.6}, zf(raw)={:.6}", initial_res, zf);
		let z = if zf.is_finite() { zf as i32 } else { 0 };
		log::trace!("Computed max level: {}", z.clamp(0, 31));
		Ok(z.clamp(0, 31) as u8)
	}
}

impl std::fmt::Debug for RasterSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RasterSource")
			.field("pool", &"<deadpool::Pool<GdalManager>>")
			.field("bbox", &self.bbox)
			.field("band_mapping", &self.band_mapping)
			.field("pixel_size", &self.pixel_size)
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

		pub fn get_factory(&self) -> Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> {
			let band_mapping_c = self.band_mapping.clone();
			let geotransform_c = self.geotransform;
			let size = self.size;
			Arc::new(move || -> Result<Dataset> {
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
}
