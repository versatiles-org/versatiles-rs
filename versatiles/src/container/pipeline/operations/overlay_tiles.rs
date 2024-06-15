use crate::{
	container::{
		pipeline::{ComposerOperationTrait, Factory, OperationTrait},
		TilesReaderParameters,
	},
	types::{Blob, TileBBox, TileCoord3, TileStream},
	utils::YamlWrapper,
};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::types::{TileBBoxPyramid, TileCompression, TileFormat};
use versatiles_derive::YamlParser;

#[derive(YamlParser)]
struct Arguments {}

#[derive(Debug)]
pub struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
}

#[async_trait]
impl ComposerOperationTrait for Operation {
	async fn new(
		yaml: YamlWrapper,
		sources: Vec<Box<dyn OperationTrait>>,
		builder: &Factory,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let args = Arguments::from_yaml(&yaml);
		let pyramid = TileBBoxPyramid::new_full(12);
		let parameters =
			TilesReaderParameters::new(TileFormat::AVIF, TileCompression::Brotli, pyramid);
		Ok(Self {
			parameters,
			sources,
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_docs() -> String {
		Arguments::generate_docs()
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_id() -> &'static str {
		"read"
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		todo!();
		//self.reader.lock().await.get_meta()
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for source in self.sources.iter() {
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
