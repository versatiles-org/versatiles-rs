//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TileSource`] interface to
//! [`TileSource`] so that the rest of the pipeline can treat it like any
//! other data source.

use super::RasterSource;
use crate::{
	PipelineFactory,
	operations::read::traits::ReadTileSource,
	traits::{OperationFactoryTrait, ReadOperationFactoryTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use std::{fmt::Debug, sync::Arc, vec};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{
	GeoBBox, TileBBox, TileBBoxPyramid, TileCompression, TileFormat, TileJSON, TileSchema, TileStream,
};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitInfo;

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
	/// How often to reuse an GDAL instances. (default: 100)
	/// Set to a lower value if you have problems like memory leaks in GDAL.
	gdal_reuse_limit: Option<u32>,
	/// The number of maximum concurrent GDAL instances to allow. (default: 4)
	/// Set to a higher value if you have enough system resources and want to increase throughput.
	gdal_concurrency_limit: Option<u8>,
}

#[derive(Debug)]
/// Concrete [`TileSource`] that merely forwards every request to an
/// underlying container [`TileSource`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	source: RasterSource,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	tile_size: u32,
}

impl Operation {
	#[context("Building from_gdal_raster operation in VPL node {:?}", vpl_node.name)]
	async fn new(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Self>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node).context("Failed to parse arguments from VPL node")?;
		log::trace!(
			"from_gdal_raster::build args: filename={:?}, tile_size={:?}, tile_format={:?}, level_min={:?}, level_max={:?}",
			args.filename,
			args.tile_size,
			args.tile_format,
			args.level_min,
			args.level_max
		);
		let filename = factory.resolve_path(&args.filename);
		log::trace!("Resolved filename: {:?}", filename);
		let source = RasterSource::new(
			&filename,
			args.gdal_reuse_limit.unwrap_or(100),
			args.gdal_concurrency_limit.unwrap_or(4) as usize,
		)
		.await?;
		let bbox = source.bbox();
		let tile_size = args.tile_size.unwrap_or(512);

		let level_max = args.level_max.unwrap_or(source.level_max(tile_size)?);
		let level_min = args.level_min.unwrap_or(level_max);
		log::trace!(
			"Building bbox pyramid: level_min={}, level_max={}, tile_size={}",
			level_min,
			level_max,
			tile_size
		);
		let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(level_min, level_max, bbox);

		let metadata = TileSourceMetadata::new(
			args.tile_format.unwrap_or(TileFormat::PNG),
			TileCompression::Uncompressed,
			bbox_pyramid,
			Traversal::ANY,
		);
		log::trace!(
			"Parameters: format={:?}, compression={:?}",
			metadata.tile_format,
			metadata.tile_compression
		);
		let mut tilejson = TileJSON {
			bounds: Some(*bbox),
			..Default::default()
		};
		metadata.update_tilejson(&mut tilejson);
		tilejson.tile_schema = Some(TileSchema::RasterRGBA);
		log::trace!("TileJSON bounds set to {:?}", tilejson.bounds);
		log::trace!("from_gdal_raster::Operation built successfully");

		Ok(Self {
			source,
			metadata,
			tilejson,
			tile_size,
		})
	}

	#[context("Failed to get image data ({width}x{height}) for bbox ({bbox:?}) from GDAL dataset")]
	async fn get_image_data_from_gdal(
		&self,
		bbox: &GeoBBox,
		width: usize,
		height: usize,
	) -> Result<Option<DynamicImage>> {
		log::debug!("get_image_data_from_gdal: bbox={:?}, size={}x{}", bbox, width, height);
		let res = self.source.get_image(bbox, width, height).await;
		match &res {
			Ok(Some(_)) => log::trace!("get_image_data_from_gdal: image available for bbox={:?}", bbox),
			Ok(None) => log::trace!("get_image_data_from_gdal: no image for bbox={:?}", bbox),
			Err(e) => log::trace!("get_image_data_from_gdal error for bbox={:?}: {}", bbox, e),
		}
		res
	}
}

impl ReadTileSource for Operation {
	#[context("Failed to build read operation")]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		Ok(Box::new(Self::new(vpl_node, factory).await?) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	/// Return the reader’s technical parameters (compression, tile size,
	/// etc.) without performing any I/O.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Expose the container's `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("gdal_raster", "gdal_raster")
	}

	/// Stream decoded raster images for all tiles within the bounding box.
	#[context("Failed to get stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);

		let count = 8192u32.div_euclid(self.tile_size).max(1);

		bbox.intersect_with_pyramid(&self.metadata.bbox_pyramid);

		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(count).collect();
		let size = self.tile_size;

		use futures::stream::{self, StreamExt};
		let streams = stream::iter(bboxes).map(move |bbox| {
			let size = size;
			async move {
				if bbox.is_empty() {
					return TileStream::empty();
				}

				let image = self
					.get_image_data_from_gdal(
						&bbox.to_geo_bbox().unwrap(),
						(size * bbox.width()) as usize,
						(size * bbox.height()) as usize,
					)
					.await
					.unwrap();

				if let Some(image) = image {
					let tile_format = self.metadata.tile_format;
					// Crop into tiles on a blocking thread
					let vec = tokio::task::spawn_blocking(move || {
						bbox
							.iter_coords()
							.filter_map(|coord| {
								image
									.crop_imm(
										(coord.x - bbox.x_min().unwrap()) * size,
										(coord.y - bbox.y_min().unwrap()) * size,
										size,
										size,
									)
									.into_optional()
									.map(|img| (coord, Tile::from_image(img, tile_format).unwrap()))
							})
							.collect::<Vec<_>>()
					})
					.await
					.unwrap();

					log::trace!("Returning {} tiles for bbox {:?}", vec.len(), bbox);
					TileStream::from_vec(vec)
				} else {
					TileStream::empty()
				}
			}
		});

		Ok(TileStream::from_streams(streams))
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
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn TileSource>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCoord;
	use versatiles_image::{DynamicImageTraitConvert, DynamicImageTraitOperation};

	fn assert_same_vec(a: &[u8], b: &[u8]) {
		assert_eq!(a.len(), b.len());
		let max: i16 = 1;
		let mut max_diff: i16 = 0;
		for i in 0..a.len() {
			max_diff = max_diff.max((a[i] as i16 - b[i] as i16).abs());
		}
		assert!(
			max_diff <= max,
			"max diff {max_diff} exceeds allowed {max} for {a:?} and {b:?}"
		);
	}

	async fn get_operation(tile_size: u32) -> Operation {
		Operation::new(
			VPLNode::try_from_str(&format!(
				"from_gdal_raster filename=\"../testdata/gradient.tif\" tile_size=\"{tile_size}\" level_min=\"0\" level_max=\"3\""
			))
			.unwrap(),
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap()
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_get_tile() -> Result<()> {
		async fn gradient_test(level: u8, x: u32, y: u32, expected_row: [u8; 7], expected_col: [u8; 7]) {
			// Build a `Operation` that points at `testdata/gradient.tif`.
			// We keep it in‑memory (no factory) and map bands 1‑2‑3 → RGB.
			let coord = TileCoord::new(level, x, y).unwrap();

			let operation = get_operation(512).await;

			// Extract a 7×7 tile and gather the RGB bytes.
			let image = operation
				.get_image_data_from_gdal(&coord.to_geo_bbox(), 7, 7)
				.await
				.unwrap()
				.unwrap();

			fn extract(cb: impl FnMut(usize) -> u8) -> Vec<u8> {
				(0..7).map(cb).collect()
			}

			// Return:
			//   [
			//     row‑3‑of‑red‑channel (x coordinate),
			//     column‑3‑of‑green‑channel (y coordinate)
			//   ]
			let pixels = image.iter_pixels().collect::<Vec<_>>();
			assert_same_vec(&extract(|i| pixels[i + 21][0]), &expected_row);
			assert_same_vec(&extract(|i| pixels[i * 7 + 3][1]), &expected_col);
		}

		// ─── zoom‑0 full‑world tile should be a uniform gradient ───

		let row = [18, 54, 91, 127, 164, 201, 237];
		let col = [12, 29, 67, 128, 188, 226, 243];
		gradient_test(0, 0, 0, row, col).await;

		// ─── zoom‑1: four quadrants of the gradient ───
		let row0 = [9, 27, 45, 64, 82, 100, 118];
		let row1 = [137, 155, 173, 192, 210, 228, 246];
		let col0 = [9, 14, 22, 34, 52, 77, 110];
		let col1 = [145, 178, 203, 221, 233, 241, 246];

		gradient_test(1, 0, 0, row0, col0).await;
		gradient_test(1, 1, 0, row1, col0).await;
		gradient_test(1, 0, 1, row0, col1).await;
		gradient_test(1, 1, 1, row1, col1).await;

		Ok(())
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_get_image_stream_returns_images() -> Result<()> {
		let operation = get_operation(16).await;
		let mut stream = operation.get_tile_stream(TileBBox::new_full(1)?).await?;
		let mut count = 0;
		while let Some((coord_out, tile)) = stream.next().await {
			let image = tile.into_image()?;
			assert_eq!(image.width(), 16);
			assert_eq!(image.height(), 16);
			let color_is = image.average_color();
			let color_should = match (coord_out.x, coord_out.y) {
				(0, 0) => [63, 43, 0],
				(1, 0) => [192, 43, 0],
				(0, 1) => [63, 212, 0],
				(1, 1) => [192, 212, 0],
				_ => panic!("Unexpected tile coordinate: {coord_out:?}"),
			};
			assert_same_vec(&color_is, &color_should);
			count += 1;
		}
		assert_eq!(count, 4);
		Ok(())
	}
}
