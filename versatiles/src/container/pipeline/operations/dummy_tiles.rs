#![allow(dead_code, unused_variables, unreachable_code)]

use crate::{
	container::{
		pipeline::{Factory, OperationTrait},
		TilesReaderParameters,
	},
	types::TileStream,
	utils::vdl::VDLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3};

#[derive(versatiles_derive::VDLDecode, Clone, Debug)]
/// generates mocked tiles
pub struct Args {
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
pub struct Operation {
	blob: Blob,
	parameters: TilesReaderParameters,
}

impl Operation {
	pub async fn new<'a>(args: Args, factory: &'a Factory) -> Result<Self> {
		Ok(Self {
			blob: todo!(),
			parameters: todo!(),
		})
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
