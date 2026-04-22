//! # From‑container read operation
//!
//! This module defines an [`Operation`] that streams tiles out of a **single
//! tile container** (e.g. `*.versatiles`, MBTiles, PMTiles, TAR bundles).
//! It adapts the container’s [`TileSource`] interface to
//! [`TileSource`] so that the rest of the pipeline can treat it like any
//! other data source.

use crate::{PipelineFactory, operations::read::traits::ReadTileSource, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_container::{DataLocation, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.
struct Args {
	/// The filename of the tile container (relative to the VPL file path), or a URL (http/https).
	/// For example: `filename="world.versatiles"` or `filename="https://example.com/world.versatiles"`.
	filename: String,
}

#[derive(Debug)]
/// Concrete [`TileSource`] that merely forwards every request to an
/// underlying container [`TileSource`].  A cached copy of the
/// container’s [`TileJSON`] metadata is kept so downstream stages can query
/// bounds and zoom levels without touching the reader again.
struct Operation {
	source: Box<dyn TileSource>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_container operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let source = factory.reader(DataLocation::try_from(&args.filename)?).await?;
		let tile_pyramid = source.tile_pyramid().await?;
		let metadata = source.metadata().clone();
		metadata.set_tile_pyramid(tile_pyramid.as_ref().clone());
		let mut tilejson = source.tilejson().clone();
		metadata.update_tilejson(&mut tilejson);

		Ok(Box::new(Self {
			source,
			metadata,
			tilejson,
		}) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> std::sync::Arc<versatiles_container::SourceType> {
		self.source.source_type()
	}

	/// Return the reader's technical parameters (compression, tile size,
	/// etc.) without performing any I/O.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Expose the container's `TileJSON` so that consumers can inspect
	/// bounds, zoom range and other dataset metadata.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<std::sync::Arc<TilePyramid>> {
		self.source.tile_pyramid().await
	}

	/// Stream raw tile blobs intersecting the bounding box by delegating to
	/// `TileSource::tile_stream`.
	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_container::tile_stream {bbox:?}");
		self.source.tile_stream(bbox).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_read_factory!("from_container", Args, Operation);

#[cfg(test)]
pub fn operation_from_reader(reader: Box<dyn TileSource>) -> Box<dyn TileSource> {
	let metadata = reader.metadata().clone();
	let mut tilejson = reader.tilejson().clone();
	metadata.update_tilejson(&mut tilejson);
	Box::new(Operation {
		source: reader,
		metadata,
		tilejson,
	}) as Box<dyn TileSource>
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
				.to_pretty_lines(10)
				.iter()
				.map(std::string::String::as_str)
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
			.tile_stream(TileBBox::from_min_and_max(3, 1, 1, 2, 3)?)
			.await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob(&Uncompressed)?.len() > 50);
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
				.to_pretty_lines(10)
				.iter()
				.map(std::string::String::as_str)
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
			.tile_stream(TileBBox::from_min_and_max(3, 1, 1, 2, 3)?)
			.await?;

		let mut n = 0;
		while let Some((coord, tile)) = stream.next().await {
			assert!(tile.into_blob(&Uncompressed)?.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.level, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}
}
