#![allow(dead_code, unused_variables, unreachable_code)]

use crate::{
	container::{
		utils::{OperationFactoryTrait, OperationTrait, PipelineFactory, ReadOperationFactoryTrait},
		TilesReaderParameters,
	},
	types::TileStream,
	utils::vpl::VPLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// generates mocked tiles
struct Args {
	/// All tile source must have the same tile format.
	format: Option<String>,
	compression: Option<String>,
	min_zoom: Option<u8>,
	max_zoom: Option<u8>,
	min_lat: Option<f32>,
	max_lat: Option<f32>,
	min_lng: Option<f32>,
	max_lng: Option<f32>,
}

#[derive(Debug)]
struct Operation {
	blob: Blob,
	parameters: TilesReaderParameters,
}

impl<'a> Operation {
	fn new(vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>>
	where
		Self: Sized + OperationTrait,
	{
		Ok(Box::new(Self {
			blob: todo!(),
			parameters: todo!(),
		}))
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_meta(&self) -> Option<Blob> {
		None
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(if self.parameters.bbox_pyramid.contains_coord(coord) {
			Some(self.blob.clone())
		} else {
			None
		})
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let coords = bbox.iter_coords().collect::<Vec<TileCoord3>>();
		TileStream::from_coord_vec_sync(coords, |c| Some((c, self.blob.clone())))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"dummy_tiles"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::new(vpl_node, factory)
	}
}
