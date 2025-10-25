use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::{BoxFuture, ready};
use std::fmt::Debug;
use versatiles_container::Tile;
use versatiles_core::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// Bounding box: [min lng, min lat, max lng, max lat].
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
				parameters.bbox_pyramid.set_level_min(level_min);
			}

			if let Some(level_max) = args.level_max {
				parameters.bbox_pyramid.set_level_max(level_max);
			}

			if let Some(bbox) = args.bbox {
				parameters.bbox_pyramid.intersect_geo_bbox(&GeoBBox::try_from(&bbox)?)?;
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

	async fn get_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);
		bbox.intersect_with_pyramid(&self.parameters.bbox_pyramid);
		Ok(self
			.source
			.get_stream(bbox)
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
	use std::collections::HashSet;

	#[tokio::test]
	async fn test_filter_inside() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(&format!("from_debug format=mvt | filter bbox=[0,0,40,20]"))
			.await?;

		let inside: &[(u8, u32, u32)] = &[
			(0, 0, 0),
			(1, 1, 0),
			(2, 2, 1),
			(3, 4, 3),
			(4, 8, 7),
			(4, 9, 7),
			(5, 16, 14),
			(5, 16, 15),
			(5, 17, 14),
			(5, 17, 15),
			(5, 18, 14),
			(5, 18, 15),
			(5, 19, 14),
			(5, 19, 15),
		];
		let set = HashSet::<(u8, u32, u32)>::from_iter(inside.iter().cloned());

		for level in 0..=5 {
			let max_xy = 1 << level;
			for x in 0..max_xy {
				for y in 0..max_xy {
					let coord = TileCoord::new(level, x, y)?;
					let count = operation.get_stream(coord.as_tile_bbox(1)?).await?.to_vec().await.len();
					if set.contains(&(level, x, y)) {
						assert!(count == 1, "Expected tile data for {coord:?} in bbox [0,0,40,20]");
					} else {
						assert!(count == 0, "Did not expect tile data for {coord:?} in bbox [0,0,40,20]");
					}
				}
			}
		}

		Ok(())
	}
}
