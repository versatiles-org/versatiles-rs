use super::{TileComposerOperation, TileComposerOperationLookup};
use crate::{
	container::{get_reader, TilesReader, TilesReaderParameters},
	types::TileStream,
	utils::YamlWrapper,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{channel::mpsc::channel, lock::Mutex, SinkExt};
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
	async fn read_stream(&self, bbox: TileBBox) -> TileStream {
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

		TileStream::from_stream(Box::pin(rx))
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
		lookup: &mut TileComposerOperationLookup,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let config = Config::from_yaml(&yaml)?;

		let reader = get_reader(&lookup.get_absolute_str(&config.filename)).await?;
		let parameters = reader.get_parameters().clone();

		Ok(ReadOperation {
			config,
			name: name.to_string(),
			parameters,
			reader: Arc::new(Mutex::new(reader)),
		})
	}

	fn get_name(&self) -> &str {
		&self.name
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		self.reader.lock().await.get_meta()
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.reader.lock().await.get_tile_data(coord).await
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		self.read_stream(bbox).await
	}
}

impl Debug for ReadOperation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ReadOperation")
			.field("name", &self.name)
			.field("filename", &self.config.filename)
			.finish()
	}
}

// Tests
#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::types::{TileCompression, TileFormat};

	#[derive(Debug)]
	struct MockTilesReader;

	fn get_test_tiles() -> Vec<(TileCoord3, Blob)> {
		vec![((0, 0, 1), vec![7, 8, 9]), ((1, 1, 1), vec![10, 11, 12])]
			.into_iter()
			.map(|(c, v)| (TileCoord3::new(c.0, c.1, c.2).unwrap(), Blob::from(v)))
			.collect()
	}

	#[async_trait]
	impl TilesReader for MockTilesReader {
		fn get_meta(&self) -> Result<Option<Blob>> {
			Ok(Some(Blob::from(vec![1, 2, 3])))
		}

		async fn get_tile_data(&mut self, _coord: &TileCoord3) -> Result<Option<Blob>> {
			Ok(Some(Blob::from(vec![4, 5, 6])))
		}

		async fn get_bbox_tile_stream(&mut self, _bbox: TileBBox) -> TileStream {
			TileStream::from_vec(get_test_tiles())
		}

		fn get_parameters(&self) -> &TilesReaderParameters {
			unimplemented!()
		}
		fn get_name(&self) -> &str {
			"mock_name"
		}
		fn get_container_name(&self) -> &str {
			"mock_container"
		}
		fn override_compression(&mut self, _tile_compression: TileCompression) {
			panic!()
		}
	}

	fn get_read_operation() -> ReadOperation {
		let mock_reader = Box::new(MockTilesReader) as Box<dyn TilesReader>;
		let reader = Arc::new(Mutex::new(mock_reader));
		let config = Config {
			filename: "mock_file".to_string(),
		};
		let parameters =
			TilesReaderParameters::new_full(TileFormat::PBF, TileCompression::Uncompressed);

		ReadOperation {
			config,
			name: "test".to_string(),
			parameters,
			reader,
		}
	}

	#[tokio::test]
	async fn test_read_stream() {
		let read_operation = get_read_operation();

		let bbox = TileBBox::new_full(1).unwrap();
		let stream = read_operation.read_stream(bbox).await;

		let result: Vec<_> = stream.collect().await;
		assert_eq!(result, get_test_tiles());
	}

	#[tokio::test]
	async fn test_get_meta() {
		let read_operation = get_read_operation();

		let meta = read_operation.get_meta().await.unwrap().unwrap();
		assert_eq!(meta.into_vec(), vec![1, 2, 3]);
	}

	#[tokio::test]
	async fn test_get_tile_data() {
		let read_operation = get_read_operation();

		let coord = TileCoord3::new(0, 0, 0).unwrap();
		let tile_data = read_operation.get_tile_data(&coord).await.unwrap().unwrap();
		assert_eq!(tile_data.into_vec(), vec![4, 5, 6]);
	}
}
