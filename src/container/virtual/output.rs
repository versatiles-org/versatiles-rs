use super::{
	operations::VirtualTileOperation,
	reader::{VOperation, VReader},
};
use crate::{
	container::TilesReader,
	types::{Blob, TileBBoxPyramid, TileCompression, TileCoord3},
	utils::{decompress, YamlWrapper},
};
use anyhow::{ensure, Context, Result};
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;

pub struct VirtualTilesOutput {
	pub input: Arc<Mutex<Box<dyn TilesReader>>>,
	pub input_compression: TileCompression,
	pub input_name: String,
	pub operations: Vec<Arc<Box<dyn VirtualTileOperation>>>,
	pub bbox_pyramid: TileBBoxPyramid,
}

impl VirtualTilesOutput {
	pub async fn new(
		def: &YamlWrapper, input_lookup: &HashMap<String, VReader>, operation_lookup: &HashMap<String, VOperation>,
	) -> Result<VirtualTilesOutput> {
		let input = def.hash_get_str("input")?;
		let input = input_lookup
			.get(input)
			.with_context(|| format!("while trying to lookup the input name"))?
			.clone();

		let input_name = input.lock().await.get_name().to_string();
		let parameters = input.lock().await.get_parameters().clone();
		let bbox_pyramid = parameters.bbox_pyramid.clone();
		let input_compression = parameters.tile_compression.clone();

		let operations = def.hash_get_value("operations")?;
		ensure!(operations.is_array(), "'operations' must be an array");
		let operations: Vec<VOperation> = operations
			.array_get_as_vec()?
			.iter()
			.map(|o| -> Result<VOperation> {
				Ok(operation_lookup
					.get(o.as_str()?)
					.with_context(|| format!("while trying to lookup the operation name"))?
					.clone())
			})
			.collect::<Result<Vec<VOperation>>>()?;

		Ok(VirtualTilesOutput {
			input,
			input_compression,
			input_name,
			operations,
			bbox_pyramid,
		})
	}
	pub async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let mut tile = if let Some(blob) = self.input.lock().await.get_tile_data(coord).await? {
			blob
		} else {
			return Ok(None);
		};

		tile = decompress(tile, &self.input_compression)?;

		for operation in self.operations.iter() {
			if let Some(blob) = operation.run(&tile)? {
				tile = blob
			} else {
				return Ok(None);
			}
		}

		Ok(Some(tile))
	}
}

impl Debug for VirtualTilesOutput {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("VirtualTilesOutput")
			.field("input", &self.input_name)
			.field("input_compression", &self.input_compression)
			.field("operations", &self.operations)
			.field("bbox_pyramid", &self.bbox_pyramid)
			.finish()
	}
}
