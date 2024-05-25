mod pbf_update_properties;

use crate::{types::Blob, utils::YamlWrapper};
use anyhow::{bail, Context, Result};
use std::fmt::Debug;

pub trait TileComposerOperation: Debug + Send + Sync {
	fn new(def: &YamlWrapper) -> Result<Self>
	where
		Self: Sized;
	fn run(&self, blob: &Blob) -> Result<Option<Blob>>;
}

pub fn new_tile_composer_operation(def: &YamlWrapper) -> Result<Box<dyn TileComposerOperation>> {
	let action = def
		.hash_get_str("action")
		.context("while parsing an action")?;

	match action {
		"pbf_replace_properties" => Ok(Box::new(
			pbf_update_properties::PBFReplacePropertiesOperation::new(def)
				.with_context(|| format!("while parsing action '{action}'"))?,
		)),
		_ => bail!("operation '{action}' is unknown"),
	}
}
