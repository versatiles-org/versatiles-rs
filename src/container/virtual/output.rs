use super::{
	operations::VirtualTileOperation,
	reader::{VOperation, VReader},
};
use crate::{container::TilesReader, utils::YamlWrapper};
use anyhow::{ensure, Context, Result};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

pub struct VirtualTilesOutput {
	input: Arc<Mutex<Box<dyn TilesReader>>>,
	operations: Vec<Arc<Box<dyn VirtualTileOperation>>>,
}

impl VirtualTilesOutput {
	pub fn new(
		def: &YamlWrapper, input_lookup: &HashMap<String, VReader>, operation_lookup: &HashMap<String, VOperation>,
	) -> Result<VirtualTilesOutput> {
		let input = def.hash_get_str("input")?;
		let input = input_lookup
			.get(input)
			.with_context(|| format!("while trying to lookup the input name"))?
			.clone();

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

		Ok(VirtualTilesOutput { input, operations })
	}
}
