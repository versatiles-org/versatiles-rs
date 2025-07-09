use crate::{traits::*, vpl::VPLNode, PipelineFactory};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::{ready, BoxFuture};
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// Bounding box: [min long, min lat, max long, max lat].
	bbox: Option<[f64; 4]>,
	/// minimal zoom level
	min: Option<u8>,
	/// maximal zoom level
	max: Option<u8>,
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
			let mut parameters = source.get_parameters().clone();

			if let Some(min) = args.min {
				parameters.bbox_pyramid.set_zoom_min(min);
			}

			if let Some(max) = args.max {
				parameters.bbox_pyramid.set_zoom_max(max);
			}

			if let Some(bbox) = args.bbox {
				parameters.bbox_pyramid.intersect_geo_bbox(&GeoBBox::from(&bbox));
			}

			let mut tilejson = source.get_tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				parameters,
				source,
				tilejson,
			}) as Box<dyn OperationTrait>)
		})
	}

	fn filter_coord(&self, coord: &TileCoord3) -> bool {
		// Check if the coordinate is within the bounding box defined in the parameters
		self.parameters.bbox_pyramid.contains_coord(coord)
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		match self.filter_coord(coord) {
			true => self.source.get_tile_data(coord).await,
			false => Ok(None),
		}
	}

	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Blob>> {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}

	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		match self.filter_coord(coord) {
			true => self.source.get_image_data(coord).await,
			false => Ok(None),
		}
	}

	async fn get_image_stream(&self, mut bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		Ok(self
			.source
			.get_image_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		match self.filter_coord(coord) {
			true => self.source.get_vector_data(coord).await,
			false => Ok(None),
		}
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
		"filter_bbox"
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

	async fn test_filter_bbox(bbox: [f64; 4], tests: Vec<(TileCoord3, bool)>) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(&format!("from_debug format=mvt | filter_bbox bbox={bbox:?}"))
			.await?;

		for (coord, expected) in tests.iter() {
			let result = operation.get_tile_data(coord).await?;
			if *expected {
				assert!(result.is_some(), "Expected tile data for {coord:?} in bbox {bbox:?}");
			} else {
				assert!(result.is_none(), "Expected no tile data for {coord:?} in bbox {bbox:?}");
			}
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_bbox_inside() {
		let bbox = [-180.0, -85.0, 180.0, 85.0];
		let tests = vec![
			(TileCoord3 { x: 1, y: 1, z: 1 }, true),
			(TileCoord3 { x: 2, y: 2, z: 2 }, true),
			(TileCoord3 { x: 3, y: 3, z: 3 }, true),
		];
		test_filter_bbox(bbox, tests).await.unwrap();
	}

	#[tokio::test]
	async fn test_filter_bbox_outside() {
		let bbox = [0.0, 0.0, 20.0, 20.0];
		let tests = vec![
			(TileCoord3 { x: 7, y: 7, z: 4 }, false),
			(TileCoord3 { x: 7, y: 8, z: 4 }, false),
			(TileCoord3 { x: 8, y: 7, z: 4 }, true),
			(TileCoord3 { x: 8, y: 8, z: 4 }, false),
		];
		test_filter_bbox(bbox, tests).await.unwrap();
	}
}
