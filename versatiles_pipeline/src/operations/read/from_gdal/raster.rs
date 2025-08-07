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
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use std::{fmt::Debug, vec};
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

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
	/// The maximum zoom level to generate tiles for. (default: 8)
	max_zoom: Option<u8>,
	/// The minimum zoom level to generate tiles for. (default: 0)
	min_zoom: Option<u8>,
}

#[derive(Debug)]
/// Concrete [`OperationTrait`] that merely forwards every request to an
/// underlying container [`TilesReaderTrait`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	dataset: super::dataset::Dataset,
	parameters: TilesReaderParameters,
	tilejson: TileJSON,
	tile_size: u32,
}

impl Operation {
	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	async fn get_image_data_from_gdal(&self, bbox: GeoBBox, width: u32, height: u32) -> Result<Option<DynamicImage>> {
		self.dataset.get_image(bbox, width, height).await
	}
}

impl ReadOperationTrait for Operation {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node).context("Failed to parse arguments from VPL node")?;
			let filename = factory.resolve_path(&args.filename);
			let dataset = super::dataset::Dataset::new(filename).await?;
			let bbox = &dataset.bbox;

			let bbox_pyramid =
				TileBBoxPyramid::from_geo_bbox(args.min_zoom.unwrap_or(0), args.max_zoom.unwrap_or(8), bbox);

			let parameters = TilesReaderParameters::new(
				args.tile_format.unwrap_or(TileFormat::PNG),
				TileCompression::Uncompressed,
				bbox_pyramid,
			);
			let mut tilejson = TileJSON {
				bounds: Some(*bbox),
				..Default::default()
			};
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				tilejson,
				parameters,
				dataset,
				tile_size: args.tile_size.unwrap_or(512),
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

	/// Retrieve the *raw* (potentially compressed) tile blob at the given
	/// coordinate; returns `Ok(None)` when the tile is missing.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		pack_image_tile(self.get_image_data(coord).await, &self.parameters)
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TilesReaderTrait::get_tile_stream`.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	/// Convenience wrapper that decodes the raw blob into an in‑memory
	/// raster image.
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		self
			.get_image_data_from_gdal(coord.as_geo_bbox(), self.tile_size, self.tile_size)
			.await
	}

	/// Stream decoded raster images for all tiles within the bounding box.
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		let count = 16384u32.div_euclid(self.tile_size).max(1);

		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(count).collect();
		Ok(
			TileStream::from_stream_iter_parallel(bboxes.into_iter().map(move |bbox| async move {
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
							(tile_coord.x - bbox.x_min) * size,
							(tile_coord.y - bbox.y_min) * size,
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
	use std::path::PathBuf;
	use versatiles_image::EnhancedDynamicImageTrait;

	async fn get_operation(tile_size: u32) -> Operation {
		Operation {
			dataset: super::super::dataset::Dataset::new(PathBuf::from("../testdata/gradient.tif"))
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
			let coord = TileCoord3::new(level, x, y).unwrap();

			let operation = get_operation(512).await;

			let blob = operation.get_tile_data(&coord).await.unwrap().unwrap();
			assert!(
				blob
					.as_slice()
					.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
			);

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
		// get_vector_data should error
		assert!(operation.get_vector_data(&TileCoord3::new(0, 0, 0)?).await.is_err());
		// get_vector_stream should error
		assert!(operation.get_vector_stream(TileBBox::new_full(4)?).await.is_err());
		Ok(())
	}
}
