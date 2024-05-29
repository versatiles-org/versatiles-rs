use super::{TileComposerOperation, TileComposerOperationLookup};
use crate::{
	container::TilesReaderParameters,
	types::{Blob, TileStream},
	utils::{decompress, YamlWrapper},
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_core::types::{TileBBox, TileCompression, TileCoord3, TileFormat};

pub trait Runner: Debug + Send + Sync {
	fn new(yaml: &YamlWrapper) -> Result<Self>
	where
		Self: Sized;
	fn run(&self, blob: Blob) -> Result<Option<Blob>>;
}

#[allow(dead_code)]
pub struct RunnerOperation<T>
where
	T: Runner,
{
	runner: Arc<T>,
	input: Box<dyn TileComposerOperation>,
	name: String,
	parameters: TilesReaderParameters,
	input_compression: TileCompression,
}

#[async_trait]
impl<T: Runner + 'static> TileComposerOperation for RunnerOperation<T> {
	async fn new(
		name: &str,
		yaml: YamlWrapper,
		lookup: &mut TileComposerOperationLookup,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let runner = Arc::new(T::new(&yaml)?);

		let input_name = yaml.hash_get_str("input")?;
		let input = lookup.construct(input_name).await?;

		let mut parameters = input.get_parameters().clone();
		ensure!(
			parameters.tile_format == TileFormat::PBF,
			"operation '{name}' needs vector tiles (PBF) from '{input_name}'",
		);

		let input_compression = parameters.tile_compression;
		parameters.tile_compression = TileCompression::Uncompressed;

		Ok(RunnerOperation {
			runner,
			input,
			input_compression,
			name: name.to_string(),
			parameters,
		})
	}

	fn get_name(&self) -> &str {
		&self.name
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

impl<T: Runner> Debug for RunnerOperation<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PBFUpdatePropertiesOperation")
			.field("name", &self.name)
			.field("input", &self.input.get_name())
			.field("input_compression", &self.input_compression)
			.field("parameters", &self.parameters)
			.field("runner", &self.runner)
			.finish()
	}
}
