use crate::{PipelineFactory, helpers::Tile, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, *};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Update metadata, see also https://github.com/mapbox/tilejson-spec/tree/master/3.0.0
struct Args {
	/// Attribution text.
	attribution: Option<String>,
	/// Description text.
	description: Option<String>,
	/// Fill zoom level.
	fillzoom: Option<u8>,
	/// Name text.
	name: Option<String>,
	/// Schema text.
	schema: Option<String>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn OperationTrait>,
	tilejson: TileJSON,
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
			let mut tilejson = source.tilejson().clone();

			if let Some(attribution) = args.attribution {
				tilejson.set_string("attribution", &attribution)?;
			}

			if let Some(description) = args.description {
				tilejson.set_string("description", &description)?;
			}

			if let Some(fillzoom) = args.fillzoom {
				tilejson.set_byte("fillzoom", fillzoom)?;
			}

			if let Some(name) = args.name {
				tilejson.set_string("name", &name)?;
			}

			if let Some(schema) = args.schema {
				tilejson.tile_schema = Some(TileSchema::try_from(schema.as_str())?);
			}

			Ok(Box::new(Self { source, tilejson }) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn parameters(&self) -> &TilesReaderParameters {
		self.source.parameters()
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);
		self.source.get_stream(bbox).await
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"meta_update"
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
mod tests {}
