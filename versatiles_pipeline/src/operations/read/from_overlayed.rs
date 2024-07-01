use crate::{
	traits::*,
	vpl::{VPLNode, VPLPipeline},
	PipelineFactory,
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};
use versatiles_core::{
	types::{Blob, TileBBox, TileCompression, TileCoord3, TileStream, TilesReaderParameters},
	utils::recompress,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Overlays multiple tile sources, using the tile from the first source that provides it.
struct Args {
	/// All tile sources must have the same format.
	sources: Vec<VPLPipeline>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	meta: Option<Blob>,
}

impl ReadOperationTrait for Operation {
	fn build(
		vpl_node: VPLNode,
		factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let sources = join_all(args.sources.into_iter().map(|c| factory.build_pipeline(c)))
				.await
				.into_iter()
				.collect::<Result<Vec<_>>>()?;

			ensure!(sources.len() > 1, "must have at least two sources");

			let meta = sources.first().unwrap().get_meta();
			let parameters = sources.first().unwrap().get_parameters();
			let mut pyramid = parameters.bbox_pyramid.clone();
			let tile_format = parameters.tile_format;
			let mut tile_compression = parameters.tile_compression;

			for source in sources.iter() {
				let parameters = source.get_parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);
				ensure!(
					parameters.tile_format == tile_format,
					"all children must have the same tile format"
				);
				if parameters.tile_compression != tile_compression {
					tile_compression = TileCompression::Uncompressed;
				}
			}

			let parameters = TilesReaderParameters::new(tile_format, tile_compression, pyramid);

			Ok(Box::new(Self {
				meta,
				parameters,
				sources,
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
		for source in self.sources.iter_mut() {
			let result = source.get_tile_data(coord).await?;
			if let Some(mut blob) = result {
				blob = recompress(
					blob,
					&source.get_parameters().tile_compression,
					&self.parameters.tile_compression,
				)?;
				return Ok(Some(blob));
			}
		}
		return Ok(None);
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let output_compression = &self.parameters.tile_compression;
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
			let mut tiles: Vec<(TileCoord3, Blob)> = Vec::new();
			for source in self.sources.iter() {
				source
					.get_bbox_tile_stream(bbox.clone())
					.await
					.for_each_sync(|(coord, mut blob)| {
						let index = bbox.get_tile_index3(&coord);
						if tiles.get(index).is_none() {
							blob = recompress(
								blob,
								&source.get_parameters().tile_compression,
								output_compression,
							)
							.unwrap();
							tiles.insert(index, (coord, blob));
						}
					})
					.await;
			}
			TileStream::from_vec(tiles)
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
		"from_overlayed"
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
