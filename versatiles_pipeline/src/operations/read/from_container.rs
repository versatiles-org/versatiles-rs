use crate::{traits::*, vpl::VPLNode, PipelineFactory};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::fmt::Debug;
use versatiles_core::types::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a tile container, such as a VersaTiles file.
struct Args {
	/// The filename of the tile container. This is relative to the path of the VPL file.
	/// For example: `filename="world.versatiles"`.
	filename: String,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	reader: Box<dyn TilesReaderTrait>,
	meta: Option<Blob>,
}

impl ReadOperationTrait for Operation {
	fn build(
		vpl_node: VPLNode,
		factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let reader = factory
				.get_reader(&factory.resolve_filename(&args.filename))
				.await?;
			let parameters = reader.get_parameters().clone();
			let meta = reader.get_meta()?;

			Ok(Box::new(Self {
				parameters,
				meta,
				reader,
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

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.reader.get_tile_data(coord).await
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> TileStream {
		self.reader.get_bbox_tile_stream(bbox).await
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_container"
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

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl("from_container filename=\"test.mbtiles\"")
			.await?;

		assert_eq!(
			&operation.get_meta().unwrap().to_string(),
			"{\"mock\":true}"
		);

		let coord = TileCoord3 { x: 2, y: 3, z: 4 };
		let blob = operation.get_tile_data(&coord).await?.unwrap();

		assert!(blob.len() > 50);

		let mut stream = operation
			.get_tile_stream(TileBBox::new(3, 1, 1, 2, 3)?)
			.await;

		let mut n = 0;
		while let Some((coord, blob)) = stream.next().await {
			assert!(blob.len() > 50);
			assert!(coord.x >= 1 && coord.x <= 2);
			assert!(coord.y >= 1 && coord.y <= 3);
			assert_eq!(coord.z, 3);
			n += 1;
		}
		assert_eq!(n, 6);

		Ok(())
	}
}
