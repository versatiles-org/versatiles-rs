use super::{Factory, OperationTrait, TransformOperationTrait};
use crate::{
	container::TilesReaderParameters,
	types::{Blob, TileStream},
	utils::{decompress, YamlWrapper},
};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_core::types::{TileBBox, TileCompression, TileCoord3, TileFormat};

pub trait RunnerTrait: Debug + Send + Sync {
	fn new(arg_yaml: &YamlWrapper, path: &std::path::Path) -> Result<Self>
	where
		Self: Sized;
	fn check_input(&self, tile_format: TileFormat, tile_compression: TileCompression) -> Result<()>;
	fn run(&self, blob: Blob) -> Result<Option<Blob>>;
	fn get_docs() -> String;
	fn get_id() -> &'static str;
}

#[allow(dead_code)]
pub struct Runner<T>
where
	T: RunnerTrait,
{
	runner: Arc<T>,
	input: Box<dyn OperationTrait>,
	parameters: TilesReaderParameters,
	input_compression: TileCompression,
}

#[async_trait]
impl<T: RunnerTrait + 'static> OperationTrait for Runner<T> {
	fn get_docs() -> String {
		T::get_docs()
	}

	fn get_id() -> &'static str {
		T::get_id()
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let compression = self.input_compression;
		let runner = self.runner.clone();

		self
			.input
			.get_bbox_tile_stream(bbox)
			.await
			.filter_map_blob_parallel(move |blob| {
				let blob = decompress(blob, &compression).unwrap();
				runner.run(blob).unwrap()
			})
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		self.input.get_meta().await
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let blob = self.input.get_tile_data(coord).await?;
		if let Some(blob) = blob {
			self.runner.run(decompress(blob, &self.input_compression)?)
		} else {
			Ok(None)
		}
	}
}

#[async_trait]
impl<T: RunnerTrait + 'static> TransformOperationTrait for Runner<T> {
	async fn new(
		yaml: YamlWrapper,
		input: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let arg_yaml = yaml.hash_get_value("arg")?;
		let runner = Arc::new(T::new(&arg_yaml, factory.get_path())?);

		let mut parameters = input.get_parameters().clone();
		runner.check_input(parameters.tile_format, parameters.tile_compression)?;

		let input_compression = parameters.tile_compression;
		parameters.tile_compression = TileCompression::Uncompressed;

		Ok(Runner {
			runner,
			input,
			input_compression,
			parameters,
		})
	}
}

impl<T: RunnerTrait> Debug for Runner<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PBFUpdatePropertiesOperation")
			.field("input_compression", &self.input_compression)
			.field("parameters", &self.parameters)
			.field("runner", &self.runner)
			.finish()
	}
}
