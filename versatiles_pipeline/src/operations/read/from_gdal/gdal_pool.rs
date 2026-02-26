use super::Instance;
use anyhow::{Result, ensure};
use deadpool::managed::{Manager, Object, Pool, RecycleResult};
use gdal::{Dataset, config::set_config_option};
use std::{ops::Deref, sync::Arc};
use versatiles_core::{GeoBBox, WORLD_SIZE, utils::float_to_int};
use versatiles_derive::context;

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
			Err(e) => Err(anyhow::anyhow!("spawn_blocking failed: {e}")),
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
pub struct GdalPool {
	pool: Pool<GdalManager>,
	bbox: GeoBBox,
	pixel_size: f64,
}

unsafe impl Sync for GdalPool {}

impl GdalPool {
	/// Create a `GdalPool` from a factory that opens a fresh GDAL `Dataset` on demand.
	///
	/// Returns the pool together with a probe `Dataset` that was opened during
	/// construction. Callers can inspect this dataset for additional metadata
	/// (e.g. band mapping) without going through the pool.
	#[context("Failed to create GDAL dataset via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
	) -> Result<(GdalPool, Dataset)> {
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
		let pixel_size = instance.get_pixel_size()?;
		log::trace!("Dataset pixel_size (m/px): {pixel_size:.6}");
		log::trace!("Dataset bbox (EPSG:4326): {bbox:?}");

		// Open a second probe dataset for callers to inspect
		// (the first one was consumed by Instance::new above).
		let probe = (open_dataset)()?;

		// Create deadpool manager and pool - single synchronization point!
		let manager = GdalManager {
			open_dataset,
			reuse_limit: reuse_limit.min(1024),
		};

		let pool = Pool::builder(manager)
			.max_size(concurrency_limit.max(1))
			.build()
			.context("failed to build deadpool")?;

		Ok((GdalPool { pool, bbox, pixel_size }, probe))
	}

	/// Get an instance from the pool.
	pub async fn get_instance(&self) -> Result<PooledInstance> {
		let obj = self
			.pool
			.get()
			.await
			.map_err(|e| anyhow::anyhow!("failed to get instance from pool: {e}"))?;
		Ok(PooledInstance(obj))
	}

	pub fn bbox(&self) -> &GeoBBox {
		&self.bbox
	}

	/// Compute the **maximum** Web-Mercator zoom level supported by this dataset's
	/// native ground resolution.
	///
	/// ## How it's computed
	/// 1. `dataset_pixel_size()` (called during construction) estimates the dataset's
	///    native resolution at the image center, **in EPSG:3857 meters per pixel**.
	/// 2. For a given `tile_size` (e.g. 256 or 512), the ground resolution at zoom 0 is
	///    `initial_res = (2pi * 6_378_137) / tile_size`.
	/// 3. The maximum zoom is:
	///
	///    ```text
	///    z_max = ceil( log2( initial_res / pixel_size_m ) )
	///    ```
	///
	///    This returns the smallest integer zoom whose nominal tile resolution is
	///    **not finer** than the dataset's native resolution.
	///
	/// The result is clamped to the range `[0, 31]`.
	#[context("Failed to compute max zoom level for tile size {tile_size}")]
	pub fn level_max(&self, tile_size: u32) -> Result<u8> {
		ensure!(tile_size > 0, "tile_size must be > 0");
		log::trace!(
			"level_max(tile_size={}) with pixel_size={:.6}",
			tile_size,
			self.pixel_size
		);

		// Initial resolution (meters per pixel at zoom 0)
		let initial_res = WORLD_SIZE / f64::from(tile_size);
		let zf = (initial_res / self.pixel_size).log2().ceil();
		log::trace!("initial_res={initial_res:.6}, zf(raw)={zf:.6}");
		let z: i32 = float_to_int(zf).unwrap_or(0);
		log::trace!("Computed max level: {}", z.clamp(0, 31));
		Ok(u8::try_from(z.clamp(0, 31))?)
	}
}

impl std::fmt::Debug for GdalPool {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("GdalPool")
			.field("pool", &"<deadpool::Pool<GdalManager>>")
			.field("bbox", &self.bbox)
			.field("pixel_size", &self.pixel_size)
			.finish()
	}
}

/// An opaque handle to a pooled GDAL `Instance`.
/// Automatically returned to the pool when dropped.
pub struct PooledInstance(Object<GdalManager>);

impl Deref for PooledInstance {
	type Target = Instance;
	fn deref(&self) -> &Instance {
		&self.0
	}
}
