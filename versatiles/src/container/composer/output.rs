use super::{operations::TileComposerOperation, TileComposerOperationLookup};
use crate::{
	container::{TilesReaderParameters, TilesStream},
	types::{Blob, TileBBox, TileCompression, TileCoord3},
	utils::{recompress, YamlWrapper},
};
use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::fmt::Debug;
use versatiles_derive::YamlParser;

/// The `TileComposerOutput` struct represents the output of the tile composition process.
/// It holds references to the input reader, compression settings, operations, and the bounding box pyramid.
pub struct TileComposerOutput {
	pub input: Box<dyn TileComposerOperation>,
	pub input_parameters: TilesReaderParameters,
	pub output_parameters: TilesReaderParameters,
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
		#[derive(YamlParser)]
		struct Definition {
			compression: Option<String>,
			input: String,
		}
		let def = Definition::from_yaml(yaml)?;

		let input = lookup.construct(&def.input).await?;

		let input_parameters = input.get_parameters().await.clone();

		let output_parameters = TilesReaderParameters::new(
			input_parameters.tile_format,
			def.compression
				.map(|s| TileCompression::parse_str(&s).unwrap())
				.unwrap_or(input_parameters.tile_compression),
			input_parameters.bbox_pyramid.clone(),
		);

		Ok(TileComposerOutput {
			input,
			input_parameters,
			output_parameters,
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
		let mut blob = if let Some(blob) = self.input.get_tile_data(coord).await? {
			blob
		} else {
			return Ok(None);
		};

		blob = recompress(
			blob,
			&self.input_parameters.tile_compression,
			&self.output_parameters.tile_compression,
		)?;

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
	pub async fn get_bbox_tile_stream(&mut self, bbox: TileBBox) -> TilesStream {
		let input_compression = &self.input_parameters.tile_compression;
		let output_compression = &self.output_parameters.tile_compression;

		self
			.input
			.get_bbox_tile_stream(bbox)
			.await
			.map(|(coord, mut blob)| {
				blob = recompress(blob, input_compression, output_compression).unwrap();
				(coord, blob)
			})
			.boxed()
	}
}

impl Debug for TileComposerOutput {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileComposerOutput")
			.field("input", &self.input)
			.field("input_parameters", &self.input_parameters)
			.field("output_parameters", &self.output_parameters)
			.finish()
	}
}
