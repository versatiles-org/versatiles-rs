use super::TileComposerOperation;
use crate::{
	container::{
		get_reader, TileComposerOperationLookup, TilesReader, TilesReaderParameters, TilesStream,
	},
	utils::YamlWrapper,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::lock::Mutex;
use std::{fmt::Debug, sync::Arc};
use versatiles_core::types::{Blob, TileBBox, TileCoord3};
use versatiles_derive::YamlParser;

#[derive(YamlParser)]
struct Config {
	filename: String,
}

/// The `ReadOperation` struct represents an operation that replaces properties in PBF tiles
/// based on a mapping provided in a CSV file.
pub struct ReadOperation {
	config: Config,
	name: String,
	parameters: TilesReaderParameters,
	reader: Arc<Mutex<Box<dyn TilesReader>>>,
}

#[async_trait]
impl TileComposerOperation for ReadOperation {
	/// Creates a new `ReadOperation` from the provided YAML configuration.
	///
	/// # Arguments
	///
	/// * `yaml` - A reference to a `YamlWrapper` containing the configuration.
	///
	/// # Returns
	///
	/// * `Result<ReadOperation>` - The constructed operation or an error if the configuration is invalid.
	async fn new(
		name: &str,
		yaml: YamlWrapper,
		_lookup: &mut TileComposerOperationLookup,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let config = Config::from_yaml(&yaml)?;

		let reader = get_reader(&config.filename).await?;
		let parameters = reader.get_parameters().clone();

		Ok(ReadOperation {
			config,
			name: name.to_string(),
			parameters,
			reader: Arc::new(Mutex::new(reader)),
		})
	}

	async fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	async fn get_meta(&self) -> Result<Option<Blob>> {
		self.reader.lock().await.get_meta()
	}
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.reader.lock().await.get_tile_data(coord).await
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TilesStream {
		self.reader.lock().await.get_bbox_tile_stream(bbox).await
	}
}

impl Debug for ReadOperation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Read")
			.field("filename", &self.config.filename)
			.finish()
	}
}
