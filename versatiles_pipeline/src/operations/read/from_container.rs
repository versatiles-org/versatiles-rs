//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TilesReaderTrait`] interface to
//! [`OperationTrait`] so that the rest of the pipeline can treat it like any
//! other data source.

use crate::{PipelineFactory, helpers::Tile, operations::read::traits::ReadOperationTrait, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::fmt::Debug;
use versatiles_core::*;

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
			let parameters = reader.parameters().clone();
			let mut tilejson = reader.tilejson().clone();
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
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Expose the container’s `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.reader.traversal()
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TilesReaderTrait::get_tile_stream`.
	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("getstream {:?}", bbox);
		let format = self.parameters.tile_format;
		let compression = self.parameters.tile_compression;
		Ok(self
			.reader
			.get_tile_stream(bbox)
			.await?
			.map_item_parallel(move |blob| Ok(Tile::from_blob(blob, format, compression))))
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
pub fn operation_from_reader(reader: Box<dyn TilesReaderTrait>) -> Box<dyn OperationTrait> {
	let parameters = reader.parameters().clone();
	let mut tilejson = reader.tilejson().clone();
	tilejson.update_from_reader_parameters(&parameters);

	Box::new(Operation {
		parameters,
		reader,
		tilejson,
	}) as Box<dyn OperationTrait>
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
				.tilejson()
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
				"  \"name\": \"dummy vector source\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		let mut stream = operation.get_stream(TileBBox::from_min_max(3, 1, 1, 2, 3)?).await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob()?.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.level, 3);
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
				.tilejson()
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
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		let mut stream = operation.get_stream(TileBBox::from_min_max(3, 1, 1, 2, 3)?).await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob()?.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.level, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}
}
