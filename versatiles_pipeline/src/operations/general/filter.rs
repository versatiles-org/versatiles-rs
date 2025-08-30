use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::{BoxFuture, ready};
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// Bounding box: [min long, min lat, max long, max lat].
	bbox: Option<[f64; 4]>,
	/// minimal zoom level
	level_min: Option<u8>,
	/// maximal zoom level
	level_max: Option<u8>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
	tilejson: TileJSON,
}

impl Operation {
	fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		_factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let mut parameters = source.parameters().clone();

			if let Some(level_min) = args.level_min {
				parameters.bbox_pyramid.set_zoom_min(level_min);
			}

			if let Some(level_max) = args.level_max {
				parameters.bbox_pyramid.set_zoom_max(level_max);
			}

			if let Some(bbox) = args.bbox {
				parameters.bbox_pyramid.intersect_geo_bbox(&GeoBBox::from(&bbox));
			}

			let mut tilejson = source.tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				parameters,
				source,
				tilejson,
			}) as Box<dyn OperationTrait>)
		})
	}

	fn filter_coord(&self, coord: &TileCoord) -> bool {
		// Check if the coordinate is within the bounding box defined in the parameters
		self.parameters.bbox_pyramid.contains_coord(coord)
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Blob>> {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}

	async fn get_image_stream(&self, mut bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(self
			.source
			.get_image_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}

	async fn get_vector_stream(&self, mut bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(self
			.source
			.get_vector_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"filter"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, source, factory).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	async fn test_filter(bbox: [f64; 4], tests: Vec<(TileCoord, bool)>) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(&format!("from_debug format=mvt | filter bbox={bbox:?}"))
			.await?;

		for (coord, expected) in tests.iter() {
			let count = operation
				.get_tile_stream(coord.as_tile_bbox(1)?)
				.await?
				.to_vec()
				.await
				.len();
			if *expected {
				assert_eq!(count, 1, "Expected tile data for {coord:?} in bbox {bbox:?}");
			} else {
				assert_eq!(count, 0, "Expected no tile data for {coord:?} in bbox {bbox:?}");
			}
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_inside() {
		let bbox = [-180.0, -85.0, 180.0, 85.0];
		let tests = vec![
			(TileCoord { x: 1, y: 1, level: 1 }, true),
			(TileCoord { x: 2, y: 2, level: 2 }, true),
			(TileCoord { x: 3, y: 3, level: 3 }, true),
		];
		test_filter(bbox, tests).await.unwrap();
	}

	#[tokio::test]
	async fn test_filter_outside() {
		let bbox = [0.0, 0.0, 20.0, 20.0];
		let tests = vec![
			(TileCoord { x: 7, y: 7, level: 4 }, false),
			(TileCoord { x: 7, y: 8, level: 4 }, false),
			(TileCoord { x: 8, y: 7, level: 4 }, true),
			(TileCoord { x: 8, y: 8, level: 4 }, false),
		];
		test_filter(bbox, tests).await.unwrap();
	}
}
