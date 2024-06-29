use crate::{
	container::{
		get_reader,
		pipeline::utils::{OperationTrait, PipelineFactory, ReadOperationFactoryTrait},
		utils::OperationFactoryTrait,
		TilesReader, TilesReaderParameters,
	},
	types::TileStream,
	utils::vpl::VPLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use futures::{future::BoxFuture, lock::Mutex};
use std::{fmt::Debug, sync::Arc};
use versatiles_core::types::{Blob, TileBBox, TileCoord3};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a tile source, such as a VersaTiles container.
struct Args {
	/// The filename of the tile container, e.g., `"world.versatiles"`.
	filename: String,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	reader: Arc<Mutex<Box<dyn TilesReader>>>,
	meta: Option<Blob>,
}

impl<'a> Operation {
	fn build(
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> BoxFuture<'a, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let reader = get_reader(&factory.resolve_filename(&args.filename)).await?;
			let parameters = reader.get_parameters().clone();
			let meta = reader.get_meta()?;

			Ok(Box::new(Self {
				parameters,
				meta,
				reader: Arc::new(Mutex::new(reader)),
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_meta(&self) -> Option<Blob> {
		self.meta.clone()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.reader.lock().await.get_tile_data(coord).await
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();
		let reader = self.reader.clone();

		TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| {
			let reader = reader.clone();
			async move {
				let tiles: Vec<(TileCoord3, Blob)> = reader
					.lock()
					.await
					.get_bbox_tile_stream(bbox.clone())
					.await
					.collect()
					.await;

				TileStream::from_vec(tiles)
			}
		}))
		.await
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"get_tiles"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory).await
	}
}
