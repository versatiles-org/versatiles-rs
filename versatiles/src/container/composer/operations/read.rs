use super::TileComposerOperation;
use crate::{
	container::{
		get_reader, TileComposerOperationLookup, TilesReader, TilesReaderParameters, TilesStream,
	},
	utils::YamlWrapper,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{channel::mpsc::channel, lock::Mutex, SinkExt, StreamExt};
use std::{fmt::Debug, sync::Arc};
use tokio::task::JoinHandle;
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

impl ReadOperation {
	async fn read_stream(&self, bbox: TileBBox) -> TilesStream {
		let (mut tx, rx) = channel::<(TileCoord3, Blob)>(64);
		let reader = self.reader.clone();

		let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
			let mut reader = reader.lock().await;
			let mut tile_stream = reader.get_bbox_tile_stream(bbox).await;

			while let Some(entry) = tile_stream.next().await {
				if tx.send(entry).await.is_err() {
					// If the receiver is dropped, break the loop
					break;
				}
			}
			Ok(())
		});

		// Optionally, you can handle the JoinHandle to ensure the task completes correctly
		tokio::spawn(async {
			if let Err(e) = handle.await {
				eprintln!("Task failed: {:?}", e);
			}
		});

		Box::pin(rx)
	}
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
		self.read_stream(bbox).await
	}
}

impl Debug for ReadOperation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Read")
			.field("name", &self.name)
			.field("filename", &self.config.filename)
			.finish()
	}
}
