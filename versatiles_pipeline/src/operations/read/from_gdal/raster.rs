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
use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use futures::future::BoxFuture;
use gdal::{Dataset, DriverManager, raster::reproject, spatial_ref::SpatialRef, vector::Geometry};
use imageproc::image::DynamicImage;
use log::warn;
use std::{fmt::Debug, path::PathBuf, vec};
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::EnhancedDynamicImageTrait;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a GDAL raster dataset and exposes it as a tile source.
struct Args {
	/// The filename of the GDAL raster dataset to read.
	/// For example: `filename="world.tif"`.
	filename: String,
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
	tile_size: usize,
	band_mapping: Vec<usize>,
}

impl Operation {
	async fn get_image_data_from_gdal(
		&self,
		bbox: GeoBBox,
		width: usize,
		height: usize,
	) -> Result<Option<DynamicImage>> {
		let src = Dataset::open(&self.filename)?;
		let channel_count = self.band_mapping.len();
		ensure!(channel_count > 0, "GDAL dataset has no bands to read");

		let driver =
			DriverManager::get_driver_by_name("MEM").context("Failed to get GDAL driver for in-memory dataset")?;
		let mut dst = driver.create_with_band_type::<u8, _>("", width, height, channel_count)?;
		dst.set_spatial_ref(&SpatialRef::from_epsg(3857)?)?;

		let mut bbox = Geometry::bbox(bbox.0, bbox.1, bbox.2, bbox.3)?;
		bbox.set_spatial_ref(SpatialRef::from_epsg(4326)?);
		bbox.transform_to_inplace(&SpatialRef::from_epsg(3857)?)?;
		let bbox = bbox.envelope();

		dst.set_geo_transform(&[
			bbox.MinX,                               // MinX
			(bbox.MaxX - bbox.MinX) / width as f64,  // Pixel width
			0.0,                                     // Rotation (should be 0)
			bbox.MinY,                               // MinY
			0.0,                                     // Rotation (should be 0)
			(bbox.MaxY - bbox.MinY) / height as f64, // Pixel height
		])?;

		reproject(&src, &dst)?;

		let mut buf = vec![0u8; width * height * channel_count];
		for (i, &band) in self.band_mapping.iter().enumerate() {
			let band_data = dst.rasterband(band)?.read_band_as::<u8>()?;
			for (j, pixel) in band_data.data().iter().enumerate() {
				buf[j * channel_count + i] = *pixel;
			}
		}

		let image = DynamicImage::from_raw(width as u32, height as u32, buf)
			.context("Failed to create DynamicImage from GDAL dataset")?;

		Ok(Some(image))
	}
}

impl ReadOperationTrait for Operation {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let filename = factory.resolve_path(&args.filename);
			let dataset = Dataset::open(&filename)?;

			let gt = dataset.geo_transform()?;
			ensure!(gt[2] == 0.0 && gt[4] == 0.0, "GDAL dataset must not be rotated");

			let width = dataset.raster_size().0;
			let height = dataset.raster_size().1;
			let spatial_ref = dataset
				.spatial_ref()
				.context(anyhow!("GDAL dataset must have a spatial reference (SRS) defined"))?;

			let mut bbox = Geometry::bbox(
				gt[0],
				gt[3],
				gt[0] + gt[1] * width as f64,
				gt[3] + gt[5] * height as f64,
			)?;
			bbox.set_spatial_ref(spatial_ref.clone());
			let mercator = SpatialRef::from_epsg(3857).unwrap();
			bbox.transform_to_inplace(&mercator).unwrap();
			let bbox = bbox.envelope();
			let bbox = GeoBBox(bbox.MinX, bbox.MinY, bbox.MaxX, bbox.MaxY);

			let max_zoom_level = 8;
			let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(0, max_zoom_level, &bbox);
			let parameters = TilesReaderParameters::new(TileFormat::PNG, TileCompression::Uncompressed, bbox_pyramid);
			let mut tilejson = TileJSON::default();
			tilejson.bounds = Some(bbox);
			tilejson.update_from_reader_parameters(&parameters);

			let mut color_index = vec![0, 0, 0];
			let mut grey_index = 0;
			let mut alpha_index = 0;
			for i in 1..=dataset.raster_count() {
				let band = dataset.rasterband(i)?;
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

			Ok(Box::new(Self {
				band_mapping,
				tilejson,
				parameters,
				filename,
				tile_size: 512,
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
		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(16).collect();
		Ok(
			TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
				let image = self
					.get_image_data_from_gdal(bbox.as_geo_bbox(), self.tile_size, self.tile_size)
					.await
					.unwrap();

				let mut tiles: Vec<Option<(TileCoord3, DynamicImage)>> = vec![];
				if image.is_some() {
					let tile_coords: Vec<TileCoord3> = bbox.iter_coords().collect();

					for tile_coord in tile_coords {
						tiles.push(Some((tile_coord, image.clone().unwrap())));
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

	#[tokio::test]
	async fn test_vector() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory.operation_from_vpl("from_gdal filename=\"test.mvt\"").await?;

		assert_eq!(
			operation
				.get_tilejson()
				.as_pretty_lines(10)
				.iter()
				.map(|s| s.as_str())
				.collect::<Vec<_>>(),
			[
				"{",
				"  \"bounds\": [",
				"    -180,",
				"    -85.051129,",
				"    180,",
				"    85.051129",
				"  ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		let coord = TileCoord3 { x: 2, y: 3, z: 4 };
		let blob = operation.get_tile_data(&coord).await?.unwrap();

		assert!(blob.len() > 50);

		let mut stream = operation.get_tile_stream(TileBBox::new(3, 1, 1, 2, 3)?).await?;

		let mut n = 0;
		while let Some((coord, blob)) = stream.next().await {
			assert!(blob.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.z, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}

	#[tokio::test]
	async fn test_raster() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory.operation_from_vpl("from_gdal filename=\"abc.png\"").await?;

		assert_eq!(
			operation
				.get_tilejson()
				.as_pretty_lines(10)
				.iter()
				.map(|s| s.as_str())
				.collect::<Vec<_>>(),
			[
				"{",
				"  \"bounds\": [",
				"    -180,",
				"    -85.051129,",
				"    180,",
				"    85.051129",
				"  ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock raster source\",",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		let coord = TileCoord3 { x: 2, y: 3, z: 4 };
		let blob = operation.get_tile_data(&coord).await?.unwrap();

		assert!(blob.len() > 50);

		let mut stream = operation.get_tile_stream(TileBBox::new(3, 1, 1, 2, 3)?).await?;

		let mut n = 0;
		while let Some((coord, blob)) = stream.next().await {
			assert!(blob.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.z, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}
}
