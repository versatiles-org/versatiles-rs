use crate::utils::YamlWrapper;
use anyhow::{bail, Context, Result};

mod replace_values;

pub trait VirtualTileOperation: Send + Sync {
	fn new(def: &YamlWrapper) -> Result<Box<dyn VirtualTileOperation>>
	where
		Self: Sized;
}

pub fn new_virtual_tile_operation(def: &YamlWrapper) -> Result<Box<dyn VirtualTileOperation>> {
	let action = def.hash_get_str("action").context("while parsing an action")?;

	(match action {
		"replace_values" => replace_values::ReplaceValuesOperation::new(def),
		_ => bail!("operation '{action}' is unknown"),
	})
	.with_context(|| format!("while parsing action '{action}'"))
}
