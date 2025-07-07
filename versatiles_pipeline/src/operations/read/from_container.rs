//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TilesReaderTrait`] interface to
//! [`OperationTrait`] so that the rest of the pipeline can treat it like any
//! other data source.

use crate::{
	helpers::{unpack_image_tile, unpack_image_tile_stream, unpack_vector_tile, unpack_vector_tile_stream},
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::VPLNode,
	PipelineFactory,
};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.
struct Args {
	/// The filename of the tile container. This is relative to the path of the VPL file.
	/// For example: `filename="world.versatiles"`.
	filename: String,
}

#[derive(Debug)]
/// Concrete [`OperationTrait`] that merely forwards every request to an
/// underlying container [`TilesReaderTrait`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	parameters: TilesReaderParameters,
	reader: Box<dyn TilesReaderTrait>,
	tilejson: TileJSON,
}

impl ReadOperationTrait for Operation {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let reader = factory.get_reader(&factory.resolve_filename(&args.filename)).await?;
			let parameters = reader.get_parameters().clone();
			let mut tilejson = reader.get_tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				tilejson,
				parameters,
				reader,
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
		self.reader.get_tile_data(coord).await
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TilesReaderTrait::get_bbox_tile_stream`.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		self.reader.get_bbox_tile_stream(bbox).await
	}

	/// Convenience wrapper that decodes the raw blob into an in‑memory
	/// raster image.
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		unpack_image_tile(self.reader.get_tile_data(coord).await, &self.parameters)
	}

	/// Stream decoded raster images for all tiles within the bounding box.
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		unpack_image_tile_stream(self.reader.get_bbox_tile_stream(bbox).await, &self.parameters)
	}

	/// Fetch and decode a single vector tile at the requested coordinate.
	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		unpack_vector_tile(self.reader.get_tile_data(coord).await, &self.parameters)
	}

	/// Stream decoded vector tiles contained in the bounding box.
	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		unpack_vector_tile_stream(self.reader.get_bbox_tile_stream(bbox).await, &self.parameters)
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_container"
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
		let operation = factory
			.operation_from_vpl("from_container filename=\"test.mvt\"")
			.await?;

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
		let operation = factory
			.operation_from_vpl("from_container filename=\"abc.png\"")
			.await?;

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
