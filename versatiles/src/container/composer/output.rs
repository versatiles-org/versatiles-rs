use super::{
	operations::TileComposerOperation,
	reader::{VOperation, VReader},
};
use crate::{
	container::{TilesReader, TilesStream},
	types::{Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3},
	utils::{compress, decompress, YamlWrapper},
};
use anyhow::{ensure, Context, Result};
use futures_util::{StreamExt, TryStreamExt};
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;

/// The `TileComposerOutput` struct represents the output of the tile composition process.
/// It holds references to the input reader, compression settings, operations, and the bounding box pyramid.
#[derive(Clone)]
pub struct TileComposerOutput {
	pub input: Arc<Mutex<Box<dyn TilesReader>>>,
	pub input_compression: TileCompression,
	pub input_name: String,
	pub operations: Vec<Arc<Box<dyn TileComposerOperation>>>,
	pub bbox_pyramid: TileBBoxPyramid,
}

impl TileComposerOutput {
	/// Creates a new `TileComposerOutput` instance from the provided YAML configuration and lookups.
	///
	/// # Arguments
	///
	/// * `def` - A reference to a `YamlWrapper` containing the configuration.
	/// * `input_lookup` - A reference to a hashmap containing input readers.
	/// * `operation_lookup` - A reference to a hashmap containing operations.
	///
	/// # Returns
	///
	/// * `Result<TileComposerOutput>` - The constructed `TileComposerOutput` or an error if the configuration is invalid.
	pub async fn new(
		def: &YamlWrapper,
		input_lookup: &HashMap<String, VReader>,
		operation_lookup: &HashMap<String, VOperation>,
	) -> Result<TileComposerOutput> {
		let input = def.hash_get_str("input")?;
		let input = input_lookup
			.get(input)
			.with_context(|| format!("Failed to lookup the input name '{input}'"))?
			.clone();

		let input_name = input.lock().await.get_name().to_string();
		let parameters = input.lock().await.get_parameters().clone();
		let bbox_pyramid = parameters.bbox_pyramid.clone();
		let input_compression = parameters.tile_compression;

		let operations = def.hash_get_value("operations")?;
		ensure!(operations.is_array(), "'operations' must be an array");
		let operations: Vec<VOperation> = operations
			.array_get_as_vec()?
			.iter()
			.map(|o| -> Result<VOperation> {
				Ok(operation_lookup
					.get(o.as_str()?)
					.with_context(|| format!("Failed to lookup the operation name '{o:?}'"))?
					.clone())
			})
			.collect::<Result<Vec<VOperation>>>()?;

		Ok(TileComposerOutput {
			input,
			input_compression,
			input_name,
			operations,
			bbox_pyramid,
		})
	}

	/// Retrieves the tile data for the specified coordinate, applying the configured operations.
	///
	/// # Arguments
	///
	/// * `coord` - A reference to the tile coordinate.
	///
	/// # Returns
	///
	/// * `Result<Option<Blob>>` - The processed tile data or `None` if the tile does not exist.
	pub async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let mut blob = if let Some(blob) = self.input.lock().await.get_tile_data(coord).await? {
			blob
		} else {
			return Ok(None);
		};

		blob = decompress(blob, &self.input_compression)?;

		for operation in self.operations.iter() {
			if let Some(new_blob) = operation.run(&blob)? {
				blob = new_blob
			} else {
				return Ok(None);
			}
		}

		Ok(Some(blob))
	}

	/// Retrieves a stream of tiles within the specified bounding box, applying the configured operations.
	///
	/// # Arguments
	///
	/// * `bbox` - The bounding box for which to retrieve the tiles.
	/// * `output_compression` - The compression format for the output tiles.
	///
	/// # Returns
	///
	/// * `TilesStream` - A stream of processed tiles.
	pub async fn get_bbox_tile_stream(
		&mut self,
		bbox: TileBBox,
		output_compression: TileCompression,
	) -> TilesStream {
		let entries: Vec<(TileCoord3, Blob)> = self
			.input
			.lock()
			.await
			.get_bbox_tile_stream(bbox)
			.await
			.collect()
			.await;

		let input_compression = self.input_compression;
		let entries: Vec<Option<(TileCoord3, Blob)>> =
			futures_util::stream::iter(entries.into_iter())
				.map(|(coord, blob)| {
					let operations = self.operations.clone();
					tokio::spawn(async move {
						let mut blob = decompress(blob, &input_compression).unwrap();

						for operation in operations.iter() {
							if let Some(new_blob) = operation.run(&blob).unwrap() {
								blob = new_blob
							} else {
								return None;
							}
						}

						blob = compress(blob, &output_compression).unwrap();

						Some((coord, blob))
					})
				})
				.buffer_unordered(num_cpus::get())
				.try_collect()
				.await
				.unwrap();

		let result = futures_util::stream::iter(entries.into_iter().flatten()).boxed();

		result
	}
}

impl Debug for TileComposerOutput {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileComposerOutput")
			.field("input", &self.input_name)
			.field("input_compression", &self.input_compression)
			.field("operations", &self.operations)
			.field("bbox_pyramid", &self.bbox_pyramid)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::{MockTilesReader, MockTilesReaderProfile};
	use std::str::FromStr;

	async fn get_mock_output() -> TileComposerOutput {
		struct MockTileComposerOperation;

		impl TileComposerOperation for MockTileComposerOperation {
			fn new(_def: &YamlWrapper) -> Result<Self>
			where
				Self: Sized,
			{
				Ok(MockTileComposerOperation)
			}

			fn run(&self, _blob: &Blob) -> Result<Option<Blob>> {
				Ok(Some(Blob::from(vec![7, 8, 9])))
			}
		}

		impl Debug for MockTileComposerOperation {
			fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
				write!(f, "MockTileComposerOperation")
			}
		}

		let input_reader: Arc<Mutex<Box<dyn TilesReader>>> = Arc::new(Mutex::new(Box::new(
			MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf).unwrap(),
		)));

		let input_name = "test_input";
		let operation_name = "test_operation";
		let operation =
			Arc::new(Box::new(MockTileComposerOperation) as Box<dyn TileComposerOperation>);

		let mut input_lookup = HashMap::new();
		input_lookup.insert(input_name.to_string(), input_reader);

		let mut operation_lookup = HashMap::new();
		operation_lookup.insert(operation_name.to_string(), operation);

		let yaml_str = format!("input: \"{input_name}\"\noperations: [\"{operation_name}\"]\n",);
		let yaml = YamlWrapper::from_str(&yaml_str).unwrap();

		TileComposerOutput::new(&yaml, &input_lookup, &operation_lookup)
			.await
			.unwrap()
	}

	#[tokio::test]
	async fn test_tile_composer_output_new() -> Result<()> {
		let composer_output = get_mock_output().await;
		assert_eq!(composer_output.input_name, "dummy_name");
		assert_eq!(composer_output.operations.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn test_tile_composer_output_get_tile_data() -> Result<()> {
		let composer_output = get_mock_output().await;
		let coord = TileCoord3::new(0, 0, 0)?;
		let result = composer_output.get_tile_data(&coord).await?;
		assert_eq!(result, Some(Blob::from(vec![7, 8, 9])));
		Ok(())
	}

	#[tokio::test]
	async fn test_tile_composer_output_get_bbox_tile_stream() -> Result<()> {
		let mut composer_output = get_mock_output().await;

		let bbox = TileBBox::new(1, 0, 0, 1, 1)?;
		let output_compression = TileCompression::None;

		let result_stream = composer_output
			.get_bbox_tile_stream(bbox, output_compression)
			.await;
		let result: Vec<(TileCoord3, Blob)> = result_stream.collect().await;

		assert_eq!(
			result,
			vec![
				(TileCoord3::new(0, 0, 1)?, Blob::from(vec![7, 8, 9])),
				(TileCoord3::new(1, 0, 1)?, Blob::from(vec![7, 8, 9])),
				(TileCoord3::new(0, 1, 1)?, Blob::from(vec![7, 8, 9])),
				(TileCoord3::new(1, 1, 1)?, Blob::from(vec![7, 8, 9]))
			]
		);
		Ok(())
	}
}
