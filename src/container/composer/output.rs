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

#[derive(Clone)]
pub struct TileComposerOutput {
	pub input: Arc<Mutex<Box<dyn TilesReader>>>,
	pub input_compression: TileCompression,
	pub input_name: String,
	pub operations: Vec<Arc<Box<dyn TileComposerOperation>>>,
	pub bbox_pyramid: TileBBoxPyramid,
}

impl TileComposerOutput {
	pub async fn new(
		def: &YamlWrapper, input_lookup: &HashMap<String, VReader>,
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
	pub async fn get_bbox_tile_stream(
		&mut self, bbox: TileBBox, output_compression: TileCompression,
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
