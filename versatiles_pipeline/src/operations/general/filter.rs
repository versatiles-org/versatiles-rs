use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSourceMetadata, TileSourceTrait, Traversal};
use versatiles_core::*;
use versatiles_derive::context;

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
	parameters: TileSourceMetadata,
	source: Box<dyn TileSourceTrait>,
	tilejson: TileJSON,
}

impl Operation {
	#[context("Building filter operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSourceTrait>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSourceTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let mut parameters = source.parameters().clone();

		if let (Some(lo), Some(hi)) = (args.level_min, args.level_max)
			&& lo > hi
		{
			bail!(
				"Invalid zoom range in filter node {:?}: level_min ({lo}) must be ≤ level_max ({hi})",
				vpl_node.name
			);
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

		if parameters.bbox_pyramid.is_empty() {
			log::warn!(
				"Filter operation in VPL node {:?} results in empty bbox_pyramid",
				vpl_node.name
			);
		}

		let mut tilejson = source.tilejson().clone();
		parameters.update_tilejson(&mut tilejson);

		Ok(Self {
			parameters,
			source,
			tilejson,
		})
	}
}

#[async_trait]
impl TileSourceTrait for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("filter", self.source.source_type())
	}

	fn parameters(&self) -> &TileSourceMetadata {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);
		bbox.intersect_with_pyramid(&self.parameters.bbox_pyramid);
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}
		self.source.get_tile_stream(bbox).await
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
		source: Box<dyn TileSourceTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSourceTrait>> {
		Operation::build(vpl_node, source, factory)
			.await
			.map(|op| Box::new(op) as Box<dyn TileSourceTrait>)
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

		let o = op.tilejson().as_object();
		assert_eq!(&o.get_number_array("bounds")?.unwrap(), &[0.0, 0.0, 40.0, 20.0]);
		assert_eq!(o.get_number("minzoom")?.unwrap(), 0.0);
		assert_eq!(o.get_number("maxzoom")?.unwrap(), 30.0);

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
		let set: HashSet<(u8, u32, u32)> = inside.iter().copied().collect();

		for level in 0..=5 {
			let max_xy = 1 << level;
			for x in 0..max_xy {
				for y in 0..max_xy {
					let coord = TileCoord::new(level, x, y)?;
					let count = op.get_tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len();
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

		let o = op.tilejson().as_object();
		assert_eq!(
			o.get_number_array("bounds")?.unwrap(),
			[-180.0, -85.051129, 180.0, 85.051129]
		);
		assert_eq!(o.get_number("minzoom")?.unwrap(), 3.0);
		assert_eq!(o.get_number("maxzoom")?.unwrap(), 4.0);

		for z in 0..=6 {
			let coord = TileCoord::new(z, 0, 0)?;
			let n = op.get_tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len();
			assert_eq!(n == 1, (3..=4).contains(&z), "z={z}");
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_invalid_zoom_range_errors() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_debug format=mvt | filter level_min=5 level_max=2")
			.await;
		assert!(result.is_err(), "expected error for level_min > level_max");
	}

	#[tokio::test]
	async fn test_filter_composition_intersection_and_zoom_narrowing() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// First filter (wider), then a second filter that further restricts bbox + zooms
		let op = factory
			.operation_from_vpl(
				"from_debug format=mvt \
             | filter bbox=[0,0,60,30] level_min=1 level_max=28 \
             | filter bbox=[10,5,40,20] level_min=3 level_max=25",
			)
			.await?;

		let o = op.tilejson().as_object();

		// Expect the intersection of the two boxes
		let b: [f64; 4] = o.get_number_array("bounds")?.unwrap();
		assert!((b[0] - 10.0).abs() < 1e-4);
		assert!((b[1] - 5.0).abs() < 1e-4);
		assert!((b[2] - 40.0).abs() < 1e-4);
		assert!((b[3] - 20.0).abs() < 1e-4);

		// Expect the narrowed zoom range
		assert_eq!(o.get_number("minzoom")?.unwrap(), 3.0);
		assert_eq!(o.get_number("maxzoom")?.unwrap(), 25.0);

		// Sanity: tiles outside the final bbox shouldn’t pass
		let outside = TileCoord::new(4, 0, 0)?.to_tile_bbox();
		let n_out = op.get_tile_stream(outside).await?.to_vec().await.len();
		assert_eq!(n_out, 0);

		// Inside tile at z=4 should pass
		let inside = TileCoord::new(4, 8, 7)?.to_tile_bbox(); // somewhere within [10,5,40,20]
		let n_in = op.get_tile_stream(inside).await?.to_vec().await.len();
		assert_eq!(n_in, 1);

		Ok(())
	}
}
