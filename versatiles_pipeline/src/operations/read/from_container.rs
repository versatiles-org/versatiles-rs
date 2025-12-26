//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TileSourceTrait`] interface to
//! [`TileSourceTrait`] so that the rest of the pipeline can treat it like any
//! other data source.

use crate::{PipelineFactory, operations::read::traits::ReadTileSourceTrait, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_container::{Tile, TileSourceMetadata, TileSourceTrait};
use versatiles_core::*;
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.
struct Args {
	/// The filename of the tile container. This is relative to the path of the VPL file.
	/// For example: `filename="world.versatiles"`.
	filename: String,
}

#[derive(Debug)]
/// Concrete [`TileSourceTrait`] that merely forwards every request to an
/// underlying container [`TileSourceTrait`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	parameters: TileSourceMetadata,
	reader: Box<dyn TileSourceTrait>,
	tilejson: TileJSON,
}

impl ReadTileSourceTrait for Operation {
	#[context("Failed to build from_container operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSourceTrait>>
	where
		Self: Sized + TileSourceTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let reader = factory.get_reader(&factory.resolve_filename(&args.filename)).await?;
		let parameters = reader.parameters().clone();
		let mut tilejson = reader.tilejson().clone();
		parameters.update_tilejson(&mut tilejson);

		Ok(Box::new(Self {
			tilejson,
			parameters,
			reader,
		}) as Box<dyn TileSourceTrait>)
	}
}

#[async_trait]
impl TileSourceTrait for Operation {
	fn source_type(&self) -> std::sync::Arc<versatiles_container::SourceType> {
		self.reader.source_type()
	}

	/// Return the reader's technical parameters (compression, tile size,
	/// etc.) without performing any I/O.
	fn parameters(&self) -> &TileSourceMetadata {
		&self.parameters
	}

	/// Expose the container's `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.reader.traversal()
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TileSourceTrait::get_tile_stream`.
	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);
		self.reader.get_tile_stream(bbox).await
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
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn TileSourceTrait>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
pub fn operation_from_reader(reader: Box<dyn TileSourceTrait>) -> Box<dyn TileSourceTrait> {
	let parameters = reader.parameters().clone();
	let mut tilejson = reader.tilejson().clone();
	parameters.update_tilejson(&mut tilejson);

	Box::new(Operation {
		parameters,
		reader,
		tilejson,
	}) as Box<dyn TileSourceTrait>
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCompression::Uncompressed;

	#[tokio::test]
	async fn test_vector() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl("from_container filename=\"test.pbf\"")
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

		let mut stream = operation
			.get_tile_stream(TileBBox::from_min_and_max(3, 1, 1, 2, 3)?)
			.await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob(Uncompressed)?.len() > 50);
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

		let mut stream = operation
			.get_tile_stream(TileBBox::from_min_and_max(3, 1, 1, 2, 3)?)
			.await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob(Uncompressed)?.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.level, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}
}
