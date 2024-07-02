use crate::{traits::*, vpl::VPLNode, PipelineFactory};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3, TileStream, TilesReaderParameters};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box.
struct Args {
	/// Bounding box: [min long, min lat, max long, max lat].
	bbox: [f64; 4],
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
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
			parameters.bbox_pyramid.intersect_geo_bbox(&args.bbox);
			Ok(Box::new(Self { parameters, source }) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_meta(&self) -> Option<Blob> {
		self.source.get_meta()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		if self.parameters.bbox_pyramid.contains_coord(coord) {
			self.source.get_tile_data(coord).await
		} else {
			Ok(None)
		}
	}

	async fn get_bbox_tile_stream(&self, mut bbox: TileBBox) -> TileStream {
		bbox.intersect_pyramid(&self.parameters.bbox_pyramid);
		self.source.get_bbox_tile_stream(bbox).await
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
