use super::{BandMapping, BandMappingItem, Instance};
use anyhow::{Context, Result, ensure};
use gdal::{Dataset, config::set_config_option, spatial_ref::SpatialRef, vector::Geometry};
use imageproc::image::DynamicImage;
use std::{
	collections::LinkedList,
	path::{Path, PathBuf},
	sync::Arc,
};
use tokio::sync::Mutex;
use versatiles_core::GeoBBox;
use versatiles_derive::context;
use versatiles_image::traits::*;

/// Web‑Mercator world circumference in meters (2πR, where R = 6,378,137 m).
/// Used to compute the ground resolution at zoom 0 for a given tile size.
const EARTH_CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * 6_378_137.0;

#[derive(Debug, Clone)]
pub struct GdalDataset {
	filename: PathBuf,
	instances: Arc<Mutex<LinkedList<Instance>>>,
	bbox: GeoBBox,
	band_mapping: Arc<BandMapping>,
	pixel_size: f64,
	max_reuse_gdal: u32,
}

unsafe impl Sync for GdalDataset {}

impl GdalDataset {
	#[context("Failed to create GDAL dataset from file {:?}", filename)]
	pub async fn new(filename: &Path, max_reuse_gdal: u32) -> Result<GdalDataset> {
		log::debug!("Opening GDAL dataset from file: {:?}", filename);

		set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;
		log::trace!("GDAL_NUM_THREADS set to ALL_CPUS");

		let mut dataset = Dataset::open(filename)?;
		log::trace!(
			"Opened GDAL dataset {:?} ({}x{}, bands={})",
			filename,
			dataset.raster_size().0,
			dataset.raster_size().1,
			dataset.raster_count()
		);

		let bbox = dataset_bbox(&dataset)?;
		let band_mapping = BandMapping::try_from(&dataset)?;
		let pixel_size = dataset_pixel_size(&dataset)?;
		log::trace!("Dataset pixel_size (m/px): {:.6}", pixel_size);

		log::trace!("Dataset bbox (EPSG:4326): {:?}", bbox);
		log::trace!("Band mapping: {band_mapping:?}");
		log::trace!("GdalDataset::new finished for {:?}", filename);

		dataset.flush_cache()?;
		let mut list = LinkedList::new();
		list.push_back(Instance::new(dataset));

		Ok(Self {
			band_mapping: Arc::new(band_mapping),
			bbox,
			filename: filename.to_path_buf(),
			instances: Arc::new(Mutex::new(list)),
			max_reuse_gdal: max_reuse_gdal.min(65536),
			pixel_size,
		})
	}

	async fn get_instance(&self) -> Instance {
		let mut instances = self.instances.lock().await;
		let instance_option: Option<Instance> = instances.pop_front();
		if let Some(instance) = instance_option {
			if instance.age() < self.max_reuse_gdal + 1 {
				return instance;
			}
			drop(instance);
		}

		Instance::new(Dataset::open(&self.filename).expect("failed to open GDAL dataset"))
	}

	async fn drop_instance(&self, mut instance: Instance) {
		instance.cleanup();
		let mut instances = self.instances.lock().await;
		instances.push_back(instance);
	}

	pub async fn get_image(&self, bbox: &GeoBBox, width: u32, height: u32) -> Result<Option<DynamicImage>> {
		let band_mapping = self.band_mapping.clone();
		let instance: Instance = self.get_instance().await;
		let dst = instance.reproject_to_dataset(width, height, bbox, band_mapping)?;
		self.drop_instance(instance).await;

		let band_mapping = self.band_mapping.clone();
		let channel_count = band_mapping.len();
		let image = tokio::task::spawn_blocking(move || -> Result<Option<DynamicImage>> {
			let mut buf = vec![0u8; (width as usize) * (height as usize) * channel_count];
			for BandMappingItem {
				channel_index,
				band_index,
			} in band_mapping.iter()
			{
				let band = dst.rasterband(band_index)?.read_band_as::<u8>()?;
				let data = band.data();
				ensure!(
					data.len() == (width as usize) * (height as usize),
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

#[context("Failed to compute bounding box for GDAL dataset")]
fn dataset_bbox(dataset: &gdal::Dataset) -> Result<GeoBBox> {
	log::trace!("Computing dataset_bbox()");
	let gt = dataset
		.geo_transform()
		.context("Failed to get geo transform from GDAL dataset")?;

	log::trace!("geo transform: {:?}", gt);

	ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

	let width = dataset.raster_size().0;
	let height = dataset.raster_size().1;
	let spatial_ref = dataset
		.spatial_ref()
		.context("GDAL dataset must have a spatial reference (SRS) defined")?;

	log::trace!("size: {}x{}", width, height);
	log::trace!("spatial reference: {:?}", &spatial_ref.to_pretty_wkt());

	let mut bbox = Geometry::bbox(
		gt[0],
		gt[3],
		gt[0] + gt[1] * width as f64,
		gt[3] + gt[5] * height as f64,
	)?;

	log::trace!("bounding box native: {:?}", bbox);

	bbox.set_spatial_ref(spatial_ref.clone());
	bbox
		.transform_to_inplace(&SpatialRef::from_epsg(4326)?)
		.context("Failed to transform bounding box to EPSG:4326")?;

	let bbox = bbox.envelope();

	log::trace!("bounding box projected: {:?}", bbox);

	// Coordinates seem to be flipped in OGREnvelope
	let mut bbox = GeoBBox::new(bbox.MinY, bbox.MinX, bbox.MaxY, bbox.MaxX);
	bbox.limit_to_mercator();

	log::trace!("bounding box: {:?}", bbox);
	Ok(bbox)
}

/// Estimate the dataset’s native pixel size **in meters/pixel (EPSG:3857)**.
///
/// Implementation details:
/// * Requires an unrotated GeoTransform.
/// * Samples the center pixel and its right/down neighbors, transforms those
///   three points to 3857, and takes the max of the two neighbor distances.
/// * Returns a strictly positive finite value or an error.
#[context("Failed to compute pixel size for GDAL dataset")]
fn dataset_pixel_size(dataset: &gdal::Dataset) -> Result<f64> {
	log::trace!("Computing dataset_pixel_size()");
	let gt = dataset
		.geo_transform()
		.context("Failed to get geo transform from GDAL dataset")?;

	// We assume no rotation (consistent with `dataset_bbox`).
	ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

	let srs = dataset
		.spatial_ref()
		.context("GDAL dataset must have a spatial reference (SRS) defined")?;

	// Helper to map pixel (col,row) to georeferenced coordinates
	let pixel_to_size = |col: f64, row: f64| -> Result<f64> {
		let point =
			|x: f64, y: f64| -> (f64, f64, f64) { (gt[0] + x * gt[1] + y * gt[2], gt[3] + x * gt[4] + y * gt[5], 0.0) };

		let mut geom = Geometry::empty(gdal_sys::OGRwkbGeometryType::wkbLineString)?;
		geom.add_point(point(col, row));
		geom.add_point(point(col + 1.0, row));
		geom.add_point(point(col, row + 1.0));
		geom.set_spatial_ref(srs.clone());
		geom.transform_to_inplace(&SpatialRef::from_epsg(3857)?)?;

		let mut p = vec![];
		geom.get_points(&mut p);

		let p0 = &p[0];
		let px = &p[1];
		let py = &p[2];
		let ax = (px.0 - p0.0).powi(2) + (px.1 - p0.1).powi(2);
		let ay = (py.0 - p0.0).powi(2) + (py.1 - p0.1).powi(2);

		Ok(ax.min(ay).sqrt())
	};

	let (width, height) = dataset.raster_size();
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

	log::trace!("pixel_size: {:.6}", size_min);
	ensure!(
		size_min.is_finite() && size_min > 0.0,
		"Invalid pixel size in meters computed"
	);
	Ok(size_min)
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::anyhow;
	use versatiles_core::TileCoord;

	#[tokio::test(flavor = "multi_thread")]
	async fn test_dataset_get_image() -> Result<()> {
		async fn gradient_test(level: u8, x: u32, y: u32) -> Result<[Vec<u8>; 2]> {
			// Build a `Operation` that points at `testdata/gradient.tif`.
			// We keep it in‑memory (no factory) and map bands 1‑2‑3 → RGB.
			let coord = TileCoord::new(level, x, y)?;

			let dataset = GdalDataset::new(&PathBuf::from("../testdata/gradient.tif"), 65535).await?;

			// Extract a 7×7 tile and gather the RGB bytes.
			let image = dataset
				.get_image(&coord.to_geo_bbox(), 7, 7)
				.await?
				.ok_or(anyhow!("get_image failed"))?;

			fn extract(mut cb: impl FnMut(usize) -> u8) -> Vec<u8> {
				(0..7)
					.map(|i| {
						let mut v = cb(i);
						if v == 64 {
							v = 63
						}
						if v == 128 {
							v = 127
						}
						if v == 192 {
							v = 191
						}
						v
					})
					.collect::<Vec<_>>()
			}

			// Return:
			//   [
			//     row‑3‑of‑red‑channel (x coordinate),
			//     column‑3‑of‑green‑channel (y coordinate)
			//   ]
			let pixels = image.iter_pixels().collect::<Vec<_>>();
			Ok([extract(|i| pixels[i + 21][0]), extract(|i| pixels[i * 7 + 3][1])])
		}

		// ─── zoom‑0 full‑world tile should be a uniform gradient ───
		assert_eq!(
			gradient_test(0, 0, 0).await?,
			[[21, 54, 91, 127, 164, 201, 234], [16, 27, 63, 127, 191, 228, 239]]
		);

		// ─── zoom‑1: four quadrants of the gradient ───
		let row0 = [10, 27, 45, 63, 82, 100, 118];
		let row1 = [137, 155, 173, 191, 210, 228, 245];
		let col0 = [10, 14, 21, 33, 51, 76, 109];
		let col1 = [146, 179, 204, 222, 234, 241, 245];

		assert_eq!(gradient_test(1, 0, 0).await?, [row0, col0]);
		assert_eq!(gradient_test(1, 1, 0).await?, [row1, col0]);
		assert_eq!(gradient_test(1, 0, 1).await?, [row0, col1]);
		assert_eq!(gradient_test(1, 1, 1).await?, [row1, col1]);

		Ok(())
	}
}
