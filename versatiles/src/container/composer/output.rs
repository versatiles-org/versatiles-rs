use super::{lookup::TileComposerOperationLookup, operations::TileComposerOperation};
use crate::{
	container::{TilesReaderParameters, TilesStream},
	types::{Blob, TileBBox, TileCoord3},
	utils::YamlWrapper,
};
use anyhow::Result;
use std::fmt::Debug;

/// The `TileComposerOutput` struct represents the output of the tile composition process.
/// It holds references to the input reader, compression settings, operations, and the bounding box pyramid.
pub struct TileComposerOutput {
	pub operation: Box<dyn TileComposerOperation>,
	pub parameters: TilesReaderParameters,
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
		yaml: &YamlWrapper,
		mut lookup: TileComposerOperationLookup,
	) -> Result<TileComposerOutput> {
		let operation_name = yaml.as_str()?;

		let operation = lookup.construct(&operation_name).await?;

		let parameters = operation.get_parameters().await.clone();

		Ok(TileComposerOutput {
			operation,
			parameters,
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
	pub async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.operation.get_tile_data(coord).await
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
	pub async fn get_bbox_tile_stream(&mut self, bbox: TileBBox) -> TilesStream {
		self.operation.get_bbox_tile_stream(bbox).await
	}
}

impl Debug for TileComposerOutput {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileComposerOutput")
			.field("operation", &self.operation)
			.field("parameters", &self.parameters)
			.finish()
	}
}
