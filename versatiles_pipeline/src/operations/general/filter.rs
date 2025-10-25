use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::fmt::Debug;
use versatiles_container::Tile;
use versatiles_core::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// Bounding box in WGS84: [min lng, min lat, max lng, max lat].
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
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let mut parameters = source.parameters().clone();

			if let (Some(lo), Some(hi)) = (args.level_min, args.level_max)
				&& lo > hi
			{
				bail!("level_min ({lo}) must be ≤ level_max ({hi})");
			}

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
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}
		Ok(self.source.get_stream(bbox).await?)
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
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20]")
			.await?;

		assert_eq!(
			&op.tilejson().as_pretty_lines(100)[0..7],
			&[
				"{",
				"  \"bounds\": [0, 0, 40, 20],",
				"  \"maxzoom\": 30,",
				"  \"minzoom\": 0,",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
			]
		);

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
					let count = op.get_stream(coord.as_tile_bbox(1)?).await?.to_vec().await.len();
					if set.contains(&(level, x, y)) {
						assert!(count == 1, "Expected one tile for {coord:?}, found {count}");
					} else {
						assert!(count == 0, "Expected no tiles for {coord:?}, found {count}");
					}
				}
			}
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_zoom_only() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter level_min=3 level_max=4")
			.await?;

		assert_eq!(
			&op.tilejson().as_pretty_lines(100)[0..7],
			&[
				"{",
				"  \"bounds\": [-180, -85.051129, 180, 85.051129],",
				"  \"maxzoom\": 4,",
				"  \"minzoom\": 3,",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\","
			]
		);

		for z in 0..=6 {
			let coord = TileCoord::new(z, 0, 0)?;
			let n = op.get_stream(coord.as_tile_bbox(1)?).await?.to_vec().await.len();
			assert_eq!(n == 1, (3..=4).contains(&z), "z={z}");
		}
		Ok(())
	}
}
