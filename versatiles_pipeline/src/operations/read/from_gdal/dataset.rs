use anyhow::{Context, Result, bail, ensure};
use gdal::{
	Dataset, DriverManager, config::set_config_option, raster::reproject, spatial_ref::SpatialRef, vector::Geometry,
};
use imageproc::image::DynamicImage;
use log::{debug, trace, warn};
use std::{
	path::{Path, PathBuf},
	sync::Arc,
};
use versatiles_core::GeoBBox;
use versatiles_derive::context;
use versatiles_image::EnhancedDynamicImageTrait;

/// Web‑Mercator world circumference in meters (2πR, where R = 6,378,137 m).
/// Used to compute the ground resolution at zoom 0 for a given tile size.
const EARTH_CIRCUMFERENCE: f64 = 2.0 * std::f64::consts::PI * 6_378_137.0;

#[derive(Debug, Clone)]
pub struct GdalDataset {
	filename: Arc<PathBuf>,
	bbox: GeoBBox,
	band_mapping: Arc<Vec<usize>>,
	pixel_size: f64,
}

unsafe impl Sync for GdalDataset {}

impl GdalDataset {
	#[context("Failed to create GDAL dataset from file {:?}", filename)]
	pub async fn new(filename: &Path) -> Result<GdalDataset> {
		set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;

		let dataset = Dataset::open(filename)?;
		let bbox = dataset_bbox(&dataset)?;
		let band_mapping = dataset_bandmapping(&dataset)?;
		let pixel_size = dataset_pixel_size(&dataset)?;

		Ok(Self {
			band_mapping: Arc::new(band_mapping),
			filename: Arc::new(filename.to_path_buf()),
			bbox,
			pixel_size,
		})
	}

	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	pub async fn get_image(&self, bbox: GeoBBox, width: u32, height: u32) -> Result<Option<DynamicImage>> {
		ensure!(width > 0 && height > 0, "Width and height must be greater than zero");

		let channel_count = self.band_mapping.len();
		ensure!(channel_count > 0, "GDAL dataset has no bands to read");

		let filename = self.filename.clone();
		let band_mapping = self.band_mapping.clone();

		let image = tokio::task::spawn_blocking(move || {
			let driver = DriverManager::get_driver_by_name("MEM").unwrap();
			let mut dst = driver
				.create_with_band_type::<u8, _>("", width as usize, height as usize, channel_count)
				.unwrap();
			dst.set_spatial_ref(&SpatialRef::from_epsg(3857).unwrap()).unwrap();

			let bbox = bbox_to_mercator(bbox);
			dst.set_geo_transform(&[
				bbox[0],                             // MinX
				(bbox[2] - bbox[0]) / width as f64,  // Pixel width
				0.0,                                 // Rotation (should be 0)
				bbox[3],                             // MinY
				0.0,                                 // Rotation (should be 0)
				(bbox[1] - bbox[3]) / height as f64, // Pixel height
			])
			.unwrap();

			let dataset =
				Dataset::open(filename.as_ref()).with_context(|| format!("Failed to open GDAL dataset {filename:?}"))?;
			reproject(&dataset, &dst).unwrap();

			let mut buf = vec![0u8; (width as usize) * (height as usize) * channel_count];
			for (i, &band) in band_mapping.iter().enumerate() {
				let band_data = dst.rasterband(band)?.read_band_as::<u8>()?;
				for (j, pixel) in band_data.data().iter().enumerate() {
					buf[j * channel_count + i] = *pixel;
				}
			}

			let image =
				DynamicImage::from_raw(width, height, buf).context("Failed to create DynamicImage from GDAL dataset")?;

			Ok::<DynamicImage, anyhow::Error>(image)
		})
		.await??;

		Ok(Some(image))
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

		// Initial resolution (meters per pixel at zoom 0)
		let initial_res = EARTH_CIRCUMFERENCE / (tile_size as f64);
		let zf = (initial_res / self.pixel_size).log2().ceil();
		let z = if zf.is_finite() { zf as i32 } else { 0 };
		Ok(z.clamp(0, 31) as u8)
	}
}

#[context("Failed to compute band mapping for GDAL dataset")]
fn dataset_bandmapping(dataset: &gdal::Dataset) -> Result<Vec<usize>> {
	let mut color_index = [0, 0, 0];
	let mut grey_index = 0;
	let mut alpha_index = 0;
	for i in 1..=dataset.raster_count() {
		let band = dataset
			.rasterband(i)
			.with_context(|| format!("Failed to get raster band {i} from GDAL dataset"))?;
		use gdal::raster::ColorInterpretation::*;
		match band.color_interpretation() {
			RedBand => color_index[0] = i,
			GreenBand => color_index[1] = i,
			BlueBand => color_index[2] = i,
			AlphaBand => alpha_index = i,
			GrayIndex => grey_index = i,
			_ => warn!(
				"GDAL band {i} has unsupported color interpretation: {:?}",
				band.color_interpretation()
			),
		}
	}

	let mut band_mapping = vec![];
	if color_index.iter().all(|&i| i > 0) {
		if grey_index > 0 {
			bail!("GDAL dataset has both color and grey bands, which is not supported");
		}
		band_mapping.push(color_index[0]);
		band_mapping.push(color_index[1]);
		band_mapping.push(color_index[2]);
	} else if grey_index > 0 {
		band_mapping.push(grey_index);
	} else {
		bail!("GDAL dataset has no color or grey bands, cannot read image data");
	}

	if alpha_index > 0 {
		band_mapping.push(alpha_index);
	}
	Ok(band_mapping)
}

#[context("Failed to compute bounding box for GDAL dataset")]
fn dataset_bbox(dataset: &gdal::Dataset) -> Result<GeoBBox> {
	let gt = dataset
		.geo_transform()
		.context("Failed to get geo transform from GDAL dataset")?;

	trace!("geo transform: {:?}", gt);

	ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

	let width = dataset.raster_size().0;
	let height = dataset.raster_size().1;
	let spatial_ref = dataset
		.spatial_ref()
		.context("GDAL dataset must have a spatial reference (SRS) defined")?;

	trace!("size: {}x{}", width, height);
	trace!("spatial reference: {:?}", &spatial_ref.to_pretty_wkt());

	let mut bbox = Geometry::bbox(
		gt[0],
		gt[3],
		gt[0] + gt[1] * width as f64,
		gt[3] + gt[5] * height as f64,
	)?;

	trace!("bounding box native: {:?}", bbox);

	bbox.set_spatial_ref(spatial_ref.clone());
	bbox
		.transform_to_inplace(&SpatialRef::from_epsg(4326)?)
		.context("Failed to transform bounding box to EPSG:4326")?;

	let bbox = bbox.envelope();

	trace!("bounding box projected: {:?}", bbox);

	// Coordinates seem to be flipped in OGREnvelope
	let mut bbox = GeoBBox::new(bbox.MinY, bbox.MinX, bbox.MaxY, bbox.MaxX);
	bbox.limit_to_mercator();

	debug!("bounding box: {:?}", bbox);
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
	let gt = dataset
		.geo_transform()
		.context("Failed to get geo transform from GDAL dataset")?;

	// We assume no rotation (consistent with `dataset_bbox`).
	ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

	let srs = dataset
		.spatial_ref()
		.context("GDAL dataset must have a spatial reference (SRS) defined")?;

	let (width, height) = dataset.raster_size();
	let cx = (width as f64) / 2.0;
	let cy = (height as f64) / 2.0;

	// Helper to map pixel (col,row) to georeferenced coordinates
	let px_to_geo = |col: f64, row: f64| -> Result<(f64, f64)> {
		let x = gt[0] + col * gt[1] + row * gt[2];
		let y = gt[3] + col * gt[4] + row * gt[5];
		let mut p = Geometry::bbox(x, y, x, y)?;
		p.set_spatial_ref(srs.clone());
		p.transform_to_inplace(&SpatialRef::from_epsg(3857)?)?;
		let e = p.envelope();
		Ok((e.MinX, e.MinY))
	};

	let (mx0, my0) = px_to_geo(cx, cy)?;
	let (mx1, my1) = px_to_geo(cx + 1.0, cy)?;
	let (mx2, my2) = px_to_geo(cx, cy + 1.0)?;

	let dist_x = ((mx1 - mx0).powi(2) + (my1 - my0).powi(2)).sqrt();
	let dist_y = ((mx2 - mx0).powi(2) + (my2 - my0).powi(2)).sqrt();
	let d = dist_x.max(dist_y);
	ensure!(d.is_finite() && d > 0.0, "Invalid pixel size in meters computed");
	Ok(d)
}

fn bbox_to_mercator(mut bbox: GeoBBox) -> [f64; 4] {
	bbox.limit_to_mercator();
	let mut bbox_geometry = Geometry::bbox(bbox.1, bbox.0, bbox.3, bbox.2).unwrap();
	bbox_geometry.set_spatial_ref(SpatialRef::from_epsg(4326).unwrap());
	bbox_geometry
		.transform_to_inplace(&SpatialRef::from_epsg(3857).unwrap())
		.with_context(|| format!("Failed to transform bounding box ({bbox:?}) to EPSG:3857"))
		.unwrap();
	let bbox = bbox_geometry.envelope();
	[bbox.MinX, bbox.MinY, bbox.MaxX, bbox.MaxY]
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCoord3;

	#[test]
	fn test_bbox_to_mercator() {
		fn run_bbox_to_mercator(bbox: [i32; 4]) -> [i32; 4] {
			let mercator_bbox = bbox_to_mercator(GeoBBox(bbox[0] as f64, bbox[1] as f64, bbox[2] as f64, bbox[3] as f64));
			[
				mercator_bbox[0] as i32,
				mercator_bbox[1] as i32,
				mercator_bbox[2] as i32,
				mercator_bbox[3] as i32,
			]
		}
		assert_eq!(
			run_bbox_to_mercator([-200, -100, 200, 100]),
			[-20037508, -20037508, 20037508, 20037508]
		);

		assert_eq!(
			run_bbox_to_mercator([-200, -1, 200, 1]),
			[-20037508, -111325, 20037508, 111325]
		);

		assert_eq!(
			run_bbox_to_mercator([-1, -100, 1, 100]),
			[-111319, -20037508, 111319, 20037508]
		);
	}

	/// End‑to‑end test that renders a **7×7 pixel crop** from the synthetic
	/// `gradient.tif` dataset at various zoom/x/y coordinates.
	/// The `gradient.tif` dataset is a simple 256x256 gradient image
	/// where the red channel contains the x‑coordinate,
	/// the green channel contains the y‑coordinate, and the blue channel is zero.
	/// This verifies:
	/// * GDAL read‑path through `get_image_data_from_gdal`,
	/// * band mapping order (R,G,B),
	/// * correct Mercator reprojection and pixel alignment.
	#[tokio::test]
	async fn test_dataset_get_image() -> Result<()> {
		async fn gradient_test(level: u8, x: u32, y: u32) -> [Vec<u8>; 2] {
			// Build a `Operation` that points at `testdata/gradient.tif`.
			// We keep it in‑memory (no factory) and map bands 1‑2‑3 → RGB.
			let coord = TileCoord3::new(level, x, y).unwrap();

			let dataset = GdalDataset::new(&PathBuf::from("../testdata/gradient.tif"))
				.await
				.unwrap();

			// Extract a 7×7 tile and gather the RGB bytes.
			let image = dataset.get_image(coord.as_geo_bbox(), 7, 7).await.unwrap().unwrap();

			fn extract(mut cb: impl FnMut(usize) -> u8) -> Vec<u8> {
				(0..7)
					.map(|i| {
						let v = cb(i);
						if v == 127 { 128 } else { v }
					})
					.collect::<Vec<_>>()
			}

			// Return:
			//   [
			//     row‑3‑of‑red‑channel (x coordinate),
			//     column‑3‑of‑green‑channel (y coordinate)
			//   ]
			let pixels = image.pixels().collect::<Vec<_>>();
			[extract(|i| pixels[i + 21][0]), extract(|i| pixels[i * 7 + 3][1])]
		}

		// ─── zoom‑0 full‑world tile should be a uniform gradient ───
		assert_eq!(
			gradient_test(0, 0, 0).await,
			[[21, 54, 91, 128, 164, 201, 234], [16, 27, 63, 128, 192, 228, 239]]
		);

		// ─── zoom‑1: four quadrants of the gradient ───
		let row0 = [10, 27, 45, 64, 82, 100, 118];
		let row1 = [137, 155, 173, 192, 210, 228, 245];
		let col0 = [10, 14, 21, 33, 51, 76, 109];
		let col1 = [146, 179, 204, 222, 234, 241, 245];

		assert_eq!(gradient_test(1, 0, 0).await, [row0, col0]);
		assert_eq!(gradient_test(1, 1, 0).await, [row1, col0]);
		assert_eq!(gradient_test(1, 0, 1).await, [row0, col1]);
		assert_eq!(gradient_test(1, 1, 1).await, [row1, col1]);

		Ok(())
	}
}
