use anyhow::{Context, Result, bail, ensure};
use gdal::{DriverManager, config::set_config_option, raster::reproject, spatial_ref::SpatialRef, vector::Geometry};
use imageproc::image::DynamicImage;
use log::warn;
use std::path::PathBuf;
use versatiles_core::types::GeoBBox;
use versatiles_derive::context;
use versatiles_image::EnhancedDynamicImageTrait;

#[derive(Debug)]
pub struct Dataset {
	dataset: gdal::Dataset,
	pub bbox: GeoBBox,
	band_mapping: Vec<usize>,
}

unsafe impl Sync for Dataset {}

impl Dataset {
	pub async fn new(filename: PathBuf) -> Result<Dataset> {
		set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;

		let dataset =
			gdal::Dataset::open(&filename).with_context(|| format!("Failed to open GDAL dataset {filename:?}"))?;

		let bbox = dataset_bbox(&dataset)?;

		let band_mapping = dataset_bandmapping(&dataset)
			.with_context(|| format!("Failed to get band mapping from GDAL dataset {filename:?}"))?;

		ensure!(
			!band_mapping.is_empty(),
			"GDAL dataset {filename:?} has no bands to read",
		);

		Ok(Self {
			band_mapping,
			dataset,
			bbox,
		})
	}

	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	pub async fn get_image(&self, bbox: GeoBBox, width: u32, height: u32) -> Result<Option<DynamicImage>> {
		let channel_count = self.band_mapping.len();
		ensure!(channel_count > 0, "GDAL dataset has no bands to read");

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

		reproject(&self.dataset, &dst).unwrap();

		let mut buf = vec![0u8; (width as usize) * (height as usize) * channel_count];
		for (i, &band) in self.band_mapping.iter().enumerate() {
			let band_data = dst.rasterband(band)?.read_band_as::<u8>()?;
			for (j, pixel) in band_data.data().iter().enumerate() {
				buf[j * channel_count + i] = *pixel;
			}
		}

		let image =
			DynamicImage::from_raw(width, height, buf).context("Failed to create DynamicImage from GDAL dataset")?;

		Ok(Some(image))
	}
}

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

fn dataset_bbox(dataset: &gdal::Dataset) -> Result<GeoBBox> {
	let gt = dataset
		.geo_transform()
		.context("Failed to get geo transform from GDAL dataset")?;

	ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

	let width = dataset.raster_size().0;
	let height = dataset.raster_size().1;
	let spatial_ref = dataset
		.spatial_ref()
		.context("GDAL dataset must have a spatial reference (SRS) defined")?;

	let mut bbox = Geometry::bbox(
		gt[3],
		gt[0],
		gt[3] + gt[5] * height as f64,
		gt[0] + gt[1] * width as f64,
	)?;
	bbox.set_spatial_ref(spatial_ref.clone());
	bbox
		.transform_to_inplace(&SpatialRef::from_epsg(4326)?)
		.context("Failed to transform bounding box to EPSG:4326")?;

	let bbox = bbox.envelope();

	let mut bbox = GeoBBox(bbox.MinX, bbox.MinY, bbox.MaxX, bbox.MaxY);
	bbox.limit_to_mercator();
	Ok(bbox)
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
	use versatiles_core::types::TileCoord3;

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
		async fn gradient_test(z: u8, x: u32, y: u32) -> [Vec<u8>; 2] {
			// Build a `Operation` that points at `testdata/gradient.tif`.
			// We keep it in‑memory (no factory) and map bands 1‑2‑3 → RGB.
			let coord = TileCoord3::new(x, y, z).unwrap();

			let dataset = Dataset::new(PathBuf::from("../testdata/gradient.tif")).await.unwrap();

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
