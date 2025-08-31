//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TilesReaderTrait`] interface to
//! [`OperationTrait`] so that the rest of the pipeline can treat it like any
//! other data source.

use crate::{
	PipelineFactory,
	helpers::pack_image_tile_stream,
	operations::read::{from_gdal::GdalDataset, traits::ReadOperationTrait},
	traits::*,
	vpl::VPLNode,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use log::{debug, trace};
use std::{fmt::Debug, vec};
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a GDAL raster dataset and exposes it as a tile source.
/// Hint: When using "gdalbuildvrt" to create a virtual raster, don't forget to set `-addalpha` option to include alpha channel.
struct Args {
	/// The filename of the GDAL raster dataset to read.
	/// For example: `filename="world.tif"`.
	filename: String,
	/// The size of the generated tiles in pixels. (default: 512)
	tile_size: Option<u32>,
	/// The tile format to use for the output tiles. (default: `PNG`)
	tile_format: Option<TileFormat>,
	/// The maximum zoom level to generate tiles for.
	/// (default: the maximum zoom level based on the dataset's native resolution)
	level_max: Option<u8>,
	/// The minimum zoom level to generate tiles for. (default: level_max)
	level_min: Option<u8>,
	/// Whether to reuse existing GDAL dataset instances. (default: true)
	/// Set to false if you have problems like memory leaks in GDAL.
	max_reuse_gdal: Option<u32>, // default: true
}

#[derive(Debug)]
/// Concrete [`OperationTrait`] that merely forwards every request to an
/// underlying container [`TilesReaderTrait`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	dataset: GdalDataset,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
	tile_size: u32,
}

impl Operation {
	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	async fn get_image_data_from_gdal(&self, bbox: GeoBBox, width: u32, height: u32) -> Result<Option<DynamicImage>> {
		trace!("get_image_data_from_gdal: bbox={:?}, size={}x{}", bbox, width, height);
		let res = self.dataset.get_image(bbox, width, height).await;
		match &res {
			Ok(Some(_)) => debug!("get_image_data_from_gdal: image available for bbox={:?}", bbox),
			Ok(None) => trace!("get_image_data_from_gdal: no image for bbox={:?}", bbox),
			Err(e) => debug!("get_image_data_from_gdal error for bbox={:?}: {}", bbox, e),
		}
		res
	}
}

impl ReadOperationTrait for Operation {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node).context("Failed to parse arguments from VPL node")?;
			debug!(
				"from_gdal_raster::build args: filename={:?}, tile_size={:?}, tile_format={:?}, level_min={:?}, level_max={:?}",
				args.filename, args.tile_size, args.tile_format, args.level_min, args.level_max
			);
			let filename = factory.resolve_path(&args.filename);
			trace!("Resolved filename: {:?}", filename);
			let dataset = GdalDataset::new(&filename, args.max_reuse_gdal.unwrap_or(u32::MAX)).await?;
			let bbox = dataset.bbox();
			let tile_size = args.tile_size.unwrap_or(512);

			let level_max = args.level_max.unwrap_or(dataset.level_max(tile_size)?);
			let level_min = args.level_min.unwrap_or(level_max);
			trace!(
				"Building bbox pyramid: level_min={}, level_max={}, tile_size={}",
				level_min, level_max, tile_size
			);
			let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(level_min, level_max, bbox);

			let parameters = TilesReaderParameters::new(
				args.tile_format.unwrap_or(TileFormat::PNG),
				TileCompression::Uncompressed,
				bbox_pyramid,
			);
			debug!(
				"Parameters: format={:?}, compression={:?}",
				parameters.tile_format, parameters.tile_compression
			);
			let mut tilejson = TileJSON {
				bounds: Some(*bbox),
				..Default::default()
			};
			tilejson.update_from_reader_parameters(&parameters);
			tilejson.tile_schema = Some(TileSchema::RasterRGBA);
			debug!("TileJSON bounds set to {:?}", tilejson.bounds);
			trace!("from_gdal_raster::Operation built successfully");

			Ok(Box::new(Self {
				tilejson,
				parameters,
				dataset,
				tile_size,
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	/// Return the reader’s technical parameters (compression, tile size,
	/// etc.) without performing any I/O.
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Expose the container’s `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TilesReaderTrait::get_tile_stream`.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	/// Stream decoded raster images for all tiles within the bounding box.
	async fn get_image_stream(&self, mut bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		let count = 8192u32.div_euclid(self.tile_size).max(1);

		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);

		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(count).collect();
		let size = self.tile_size;

		use futures::stream::{self, StreamExt};
		let streams = stream::iter(bboxes).map(move |bbox| {
			let size = size;
			async move {
				let image = self
					.get_image_data_from_gdal(bbox.as_geo_bbox(), size * bbox.width(), size * bbox.height())
					.await
					.unwrap();

				if let Some(image) = image {
					// Crop into tiles on a blocking thread
					let vec = tokio::task::spawn_blocking(move || {
						bbox
							.iter_coords()
							.filter_map(|coord| {
								image
									.crop_imm((coord.x - bbox.x_min) * size, (coord.y - bbox.y_min) * size, size, size)
									.into_optional()
									.map(|img| (coord, img))
							})
							.collect::<Vec<_>>()
					})
					.await
					.unwrap();

					debug!("Returning {} tiles for bbox {:?}", vec.len(), bbox);
					TileStream::from_vec(vec)
				} else {
					TileStream::new_empty()
				}
			}
		});

		Ok(TileStream::from_streams(streams, 4))
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
	use std::path::PathBuf;

	async fn get_operation(tile_size: u32) -> Operation {
		Operation {
			dataset: GdalDataset::new(&PathBuf::from("../testdata/gradient.tif"), 65535)
				.await
				.unwrap(),
			parameters: TilesReaderParameters::new(
				TileFormat::PNG,
				TileCompression::Uncompressed,
				TileBBoxPyramid::new_full(4),
			),
			tilejson: TileJSON::default(),
			tile_size,
		}
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		async fn gradient_test(level: u8, x: u32, y: u32) -> [Vec<u8>; 2] {
			// Build a `Operation` that points at `testdata/gradient.tif`.
			// We keep it in‑memory (no factory) and map bands 1‑2‑3 → RGB.
			let coord = TileCoord::new(level, x, y).unwrap();

			let operation = get_operation(512).await;

			// Extract a 7×7 tile and gather the RGB bytes.
			let image = operation
				.get_image_data_from_gdal(coord.as_geo_bbox(), 7, 7)
				.await
				.unwrap()
				.unwrap();

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
			let pixels = image.iter_pixels().collect::<Vec<_>>();
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

	#[tokio::test]
	async fn test_get_image_stream_returns_images() -> Result<()> {
		let operation = get_operation(16).await;
		let mut stream = operation.get_image_stream(TileBBox::new_full(1)?).await?;
		let mut count = 0;
		while let Some((coord_out, image)) = stream.next().await {
			assert_eq!(image.width(), 16);
			assert_eq!(image.height(), 16);
			let color_is = image.average_color();
			let color_should = match (coord_out.x, coord_out.y) {
				(0, 0) => [64, 43, 0],
				(1, 0) => [192, 43, 0],
				(0, 1) => [64, 212, 0],
				(1, 1) => [192, 212, 0],
				_ => panic!("Unexpected tile coordinate: {coord_out:?}"),
			};
			assert_eq!(
				color_is, color_should,
				"Tile at {coord_out:?} has unexpected average color: {color_is:?} (should be {color_should:?})",
			);
			count += 1;
		}
		assert_eq!(count, 4);
		Ok(())
	}

	#[tokio::test]
	async fn test_vector_methods_error() -> Result<()> {
		let operation = get_operation(512).await;
		// get_vector_stream should error
		assert!(operation.get_vector_stream(TileBBox::new_full(4)?).await.is_err());
		Ok(())
	}
}
