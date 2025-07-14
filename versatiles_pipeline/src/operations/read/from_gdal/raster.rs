//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TilesReaderTrait`] interface to
//! [`OperationTrait`] so that the rest of the pipeline can treat it like any
//! other data source.

use crate::{
	PipelineFactory,
	helpers::{pack_image_tile, pack_image_tile_stream},
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::VPLNode,
};
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use futures::future::BoxFuture;
use gdal::{
	Dataset, DriverManager, config::set_config_option, raster::reproject, spatial_ref::SpatialRef, vector::Geometry,
};
use imageproc::image::DynamicImage;
use log::warn;
use std::{fmt::Debug, path::PathBuf, vec};
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::EnhancedDynamicImageTrait;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a GDAL raster dataset and exposes it as a tile source.
struct Args {
	/// The filename of the GDAL raster dataset to read.
	/// For example: `filename="world.tif"`.
	filename: String,
	/// The size of the generated tiles in pixels. (default: 512)
	tile_size: Option<u32>,
	/// The tile format to use for the output tiles. (default: `PNG`)
	tile_format: Option<TileFormat>,
}

#[derive(Debug)]
/// Concrete [`OperationTrait`] that merely forwards every request to an
/// underlying container [`TilesReaderTrait`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	filename: PathBuf,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
	tile_size: u32,
	band_mapping: Vec<usize>,
}

impl Operation {
	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	async fn get_image_data_from_gdal(&self, bbox: GeoBBox, width: u32, height: u32) -> Result<Option<DynamicImage>> {
		let src = Dataset::open(&self.filename)
			.with_context(|| format!("Failed to open GDAL dataset {}", self.filename.display()))?;

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

		reproject(&src, &dst).unwrap();

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

fn bbox_to_mercator(mut bbox: GeoBBox) -> [f64; 4] {
	bbox.limit_to_mercator();
	let mut bbox_geometry = Geometry::bbox(bbox.1, bbox.0, bbox.3, bbox.2).unwrap();
	bbox_geometry.set_spatial_ref(SpatialRef::from_epsg(4326).unwrap());
	bbox_geometry
		.transform_to_inplace(&SpatialRef::from_epsg(3857).unwrap())
		.with_context(|| format!("Failed to transform bounding box ({:?}) to EPSG:3857", bbox))
		.unwrap();
	let bbox = bbox_geometry.envelope();
	[bbox.MinX, bbox.MinY, bbox.MaxX, bbox.MaxY]
}

fn dataset_bbox(dataset: &Dataset) -> Result<GeoBBox> {
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

fn dataset_bandmapping(dataset: &Dataset) -> Result<Vec<usize>> {
	let mut color_index = vec![0, 0, 0];
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

impl ReadOperationTrait for Operation {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			set_config_option("GDAL_NUM_THREADS", "ALL_CPUS")?;

			let args = Args::from_vpl_node(&vpl_node).context("Failed to parse arguments from VPL node")?;
			let filename = factory.resolve_path(&args.filename);
			let dataset =
				Dataset::open(&filename).with_context(|| format!("Failed to open GDAL dataset {:?}", filename))?;

			let bbox = dataset_bbox(&dataset)?;

			let max_zoom_level = 8;
			let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(0, max_zoom_level, &bbox);
			let parameters = TilesReaderParameters::new(
				args.tile_format.unwrap_or(TileFormat::PNG),
				TileCompression::Uncompressed,
				bbox_pyramid,
			);
			let mut tilejson = TileJSON::default();
			tilejson.bounds = Some(bbox);
			tilejson.update_from_reader_parameters(&parameters);

			let band_mapping = dataset_bandmapping(&dataset)
				.with_context(|| format!("Failed to get band mapping from GDAL dataset {:?}", filename))?;

			ensure!(
				!band_mapping.is_empty(),
				"GDAL dataset {} has no bands to read",
				filename.display()
			);

			Ok(Box::new(Self {
				band_mapping,
				tilejson,
				parameters,
				filename,
				tile_size: args.tile_size.unwrap_or(512),
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	/// Return the reader’s technical parameters (compression, tile size,
	/// etc.) without performing any I/O.
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Expose the container’s `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn get_tilejson(&self) -> &TileJSON {
		&self.tilejson
	}
	/// Retrieve the *raw* (potentially compressed) tile blob at the given
	/// coordinate; returns `Ok(None)` when the tile is missing.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		pack_image_tile(self.get_image_data(coord).await, &self.parameters)
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TilesReaderTrait::get_bbox_tile_stream`.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	/// Convenience wrapper that decodes the raw blob into an in‑memory
	/// raster image.
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		self
			.get_image_data_from_gdal(coord.as_geo_bbox(), self.tile_size, self.tile_size)
			.await
			.with_context(|| {
				format!(
					"Failed to decode image tile at {coord:?} from GDAL dataset {}",
					self.filename.display()
				)
			})
	}

	/// Stream decoded raster images for all tiles within the bounding box.
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		let count = 16384u32.div_euclid(self.tile_size).max(1);

		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(count).collect();
		Ok(
			TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
				let size = self.tile_size;

				let image = self
					.get_image_data_from_gdal(bbox.as_geo_bbox(), size * bbox.width(), size * bbox.height())
					.await
					.unwrap();

				let mut tiles: Vec<Option<(TileCoord3, DynamicImage)>> = vec![];
				if let Some(image) = image {
					let tile_coords: Vec<TileCoord3> = bbox.iter_coords().collect();

					for tile_coord in tile_coords {
						let tile = image.crop_imm(
							(tile_coord.x - bbox.x_min) as u32 * size,
							(tile_coord.y - bbox.y_min) as u32 * size,
							size,
							size,
						);
						tiles.push(Some((tile_coord, tile)));
					}
				}
				TileStream::from_vec(tiles.into_iter().flatten().collect())
			}))
			.await,
		)
	}

	/// Fetch and decode a single vector tile at the requested coordinate.
	async fn get_vector_data(&self, _coord: &TileCoord3) -> Result<Option<VectorTile>> {
		bail!("Vector tiles are not supported in operation `from_gdal_raster`")
	}

	/// Stream decoded vector tiles contained in the bounding box.
	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in operation `from_gdal_raster`")
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_gdal_raster"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
	async fn test_operation_get_tile_data() -> Result<()> {
		async fn gradient_test(z: u8, x: u32, y: u32) -> [Vec<u8>; 2] {
			// Build a `Operation` that points at `testdata/gradient.tif`.
			// We keep it in‑memory (no factory) and map bands 1‑2‑3 → RGB.
			let coord = TileCoord3::new(x, y, z).unwrap();

			let operation = Operation {
				filename: PathBuf::from("../testdata/gradient.tif"),
				parameters: TilesReaderParameters::new(
					TileFormat::PNG,
					TileCompression::Uncompressed,
					TileBBoxPyramid::new_full(2),
				),
				tilejson: TileJSON::default(),
				tile_size: 512,
				band_mapping: vec![1, 2, 3],
			};

			// Extract a 7×7 tile and gather the RGB bytes.
			let image = operation
				.get_image_data_from_gdal(coord.as_geo_bbox(), 7, 7)
				.await
				.unwrap()
				.unwrap();

			// Return:
			//   [
			//     row‑3‑of‑red‑channel (x coordinate),
			//     column‑3‑of‑green‑channel (y coordinate)
			//   ]
			let pixels = image.pixels().collect::<Vec<_>>();
			[
				(0..7).map(|i| pixels[i + 21][0]).collect(),
				(0..7).map(|i| pixels[i * 7 + 3][1]).collect(),
			]
		}

		// ─── zoom‑0 full‑world tile should be a uniform gradient ───
		assert_eq!(
			gradient_test(0, 0, 0).await,
			[[21, 54, 91, 128, 164, 201, 234], [16, 27, 63, 127, 192, 228, 239]]
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
