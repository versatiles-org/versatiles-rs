use crate::{
	container::{
		pipeline::{Factory, OperationKDLEnum, OperationTrait},
		TilesReaderParameters,
	},
	types::{Blob, TileBBox, TileCoord3, TileStream},
	utils::KDLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};
use versatiles_core::types::{TileBBoxPyramid, TileCompression, TileFormat};

#[derive(versatiles_derive::KDLDecode, Clone, Debug)]
/// Overlays multiple tile sources. The tile of the first source that returns a tile is used.
pub struct OverlayTilesOperationKDL {
	/// All tile source must have the same tile format.
	children: Vec<OperationKDLEnum>,
}

#[derive(Debug)]
pub struct OverlayTilesOperation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
}

impl<'a> OverlayTilesOperation {
	pub fn new(node: OverlayTilesOperationKDL, factory: &'a Factory) -> BoxFuture<'a, Result<Self>> {
		Box::pin(async move {
			let sources = join_all(
				node
					.children
					.into_iter()
					.map(|c| factory.build_operation(c)),
			)
			.await
			.into_iter()
			.collect::<Result<Vec<_>>>()?;

			let pyramid = TileBBoxPyramid::new_full(12);
			let parameters =
				TilesReaderParameters::new(TileFormat::AVIF, TileCompression::Brotli, pyramid);
			Ok(Self {
				parameters,
				sources,
			})
		})
	}
}

#[async_trait]
impl OperationTrait for OverlayTilesOperation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_meta(&self) -> Option<Blob> {
		todo!();
		//self.reader.lock().await.get_meta()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for source in self.sources.iter_mut() {
			let result = source.get_tile_data(coord).await?;
			if result.is_some() {
				return Ok(result);
			}
		}
		return Ok(None);
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();
		TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
			let mut tiles: Vec<(TileCoord3, Blob)> = Vec::new();
			for source in self.sources.iter() {
				source
					.get_bbox_tile_stream(bbox.clone())
					.await
					.for_each_sync(|e| {
						let index = bbox.get_tile_index3(&e.0);
						if tiles.get(index).is_none() {
							tiles.insert(index, e);
						}
					})
					.await;
			}
			TileStream::from_vec(tiles)
		}))
		.await
	}
}
