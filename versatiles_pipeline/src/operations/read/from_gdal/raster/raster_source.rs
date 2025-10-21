use super::{BandMapping, BandMappingItem, Instance};
use anyhow::{Context, Result, ensure};
use gdal::{Dataset, config::set_config_option};
use imageproc::image::DynamicImage;
use std::{
	collections::LinkedList,
	path::{Path, PathBuf},
	sync::Arc,
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use versatiles_core::GeoBBox;
use versatiles_derive::context;
use versatiles_image::traits::*;

/// Web‑Mercator world circumference in meters (2πR, where R = 6,378,137 m).
/// Used to compute the ground resolution at zoom 0 for a given tile size.
const EARTH_CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * 6_378_137.0;

#[derive(Debug, Clone)]
pub struct RasterSource {
	filename: PathBuf,
	instances: Arc<Mutex<LinkedList<Instance>>>,
	bbox: GeoBBox,
	band_mapping: Arc<BandMapping>,
	pixel_size: f64,
	reuse_limit: u32,
	/// Limits the maximum number of concurrently checked‑out `Instance`s.
	sem: Arc<Semaphore>,
}

unsafe impl Sync for RasterSource {}

/// An `Instance` checked out from the pool while holding a semaphore permit.
struct HeldInstance {
	inst: Instance,
	_permit: OwnedSemaphorePermit,
}

impl RasterSource {
	#[context("Failed to create GDAL dataset from file {:?}", filename)]
	pub async fn new(filename: &Path, reuse_limit: u32, concurrency_limit: usize) -> Result<RasterSource> {
		log::debug!("Opening GDAL dataset from file: {:?}", filename);

		set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;
		log::trace!("GDAL_NUM_THREADS set to ALL_CPUS");

		let dataset = Dataset::open(filename)?;
		log::trace!(
			"Opened GDAL dataset {:?} ({}x{}, bands={})",
			filename,
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
		log::trace!("GdalDataset::new finished for {:?}", filename);

		let mut list = LinkedList::new();
		list.push_back(instance);

		Ok(Self {
			band_mapping: Arc::new(band_mapping),
			bbox,
			filename: filename.to_path_buf(),
			instances: Arc::new(Mutex::new(list)),
			reuse_limit: reuse_limit.min(1024),
			pixel_size,
			sem: Arc::new(Semaphore::new(concurrency_limit.max(1))),
		})
	}

	async fn get_instance(&self) -> HeldInstance {
		let permit = self.sem.clone().acquire_owned().await.expect("semaphore closed");

		let inst = {
			let mut instances = self.instances.lock().await;
			if let Some(instance) = instances.pop_front()
				&& instance.age() < self.reuse_limit + 1
			{
				instance
			} else {
				Instance::new(Dataset::open(&self.filename).expect("failed to open GDAL dataset"))
			}
		};

		HeldInstance { inst, _permit: permit }
	}

	async fn drop_instance(&self, mut held: HeldInstance) {
		held.inst.cleanup();
		let mut instances = self.instances.lock().await;
		instances.push_back(held.inst);
		// `_permit` drops here, releasing one concurrency slot
	}

	pub async fn get_image(&self, bbox: &GeoBBox, width: usize, height: usize) -> Result<Option<DynamicImage>> {
		let band_mapping = self.band_mapping.clone();

		let held = self.get_instance().await;
		let dst = held.inst.reproject_to_dataset(width, height, bbox, band_mapping)?;
		self.drop_instance(held).await;

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

#[cfg(test)]
mod tests {
	use super::super::get_spatial_ref;
	use super::*;
	use gdal::DriverManager;
	use imageproc::image::ColorType;
	use rstest::rstest;
	use std::vec;

	impl RasterSource {
		pub fn from_testdata(bbox: GeoBBox, channel_count: usize) -> Result<RasterSource> {
			let size = 256;
			let band_mapping = {
				let mut v = vec![];
				for channel_index in 0..channel_count {
					v.push(channel_index + 1);
				}
				BandMapping::new(v)
			};

			let driver = DriverManager::get_driver_by_name("MEM")?;
			let mut ds_src = driver.create_with_band_type::<u8, _>("in memory dataset", size, size, channel_count)?;
			ds_src.set_spatial_ref(&get_spatial_ref(4326)?)?;
			let geotransform = [
				bbox.x_min,
				(bbox.x_max - bbox.x_min) / size as f64,
				0.0,
				bbox.y_max,
				0.0,
				(bbox.y_min - bbox.y_max) / size as f64,
			];
			ds_src.set_geo_transform(&geotransform)?;

			let mut parameters = vec![];
			for band_index in 1..=channel_count {
				parameters.push(versatiles_image::MarkerParameters {
					offset: 0.0,
					scale: 200.0,
					angle: 80.0 + (band_index as f64 - 1.0) * 90.0,
				})
			}
			let image = DynamicImage::new_marker(&parameters);

			for c in 1..=channel_count {
				let mut band = ds_src.rasterband(c)?;
				let data = image.iter_pixels().map(|p| p[c - 1]).collect();
				let mut buffer = gdal::raster::Buffer::new((size, size), data);
				band.write((0, 0), (size, size), &mut buffer)?;

				use gdal::raster::ColorInterpretation::*;
				let interp = match channel_count {
					1 => GrayIndex,
					2 => match c {
						1 => GrayIndex,
						2 => AlphaBand,
						_ => unreachable!(),
					},
					3 => match c {
						1 => RedBand,
						2 => GreenBand,
						3 => BlueBand,
						_ => unreachable!(),
					},
					4 => match c {
						1 => RedBand,
						2 => GreenBand,
						3 => BlueBand,
						4 => AlphaBand,
						_ => unreachable!(),
					},
					_ => unreachable!(),
				};
				band.set_color_interpretation(interp)?;
			}

			Ok(RasterSource {
				filename: PathBuf::from("in-memory"),
				instances: Arc::new(Mutex::new(LinkedList::from([Instance::new(ds_src)]))),
				band_mapping: Arc::new(band_mapping),
				bbox,
				pixel_size: 1.0,
				reuse_limit: 1,
				sem: Arc::new(Semaphore::new(2)),
			})
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
