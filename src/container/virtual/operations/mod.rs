use crate::{types::Blob, utils::YamlWrapper};
use anyhow::{bail, Context, Result};

mod pbf_replace_properties;

pub trait VirtualTileOperation: Send + Sync {
	fn new(def: &YamlWrapper) -> Result<Box<dyn VirtualTileOperation>>
	where
		Self: Sized;
	fn run(&self, blob: &Blob) -> Result<Option<Blob>>;
}

pub fn new_virtual_tile_operation(def: &YamlWrapper) -> Result<Box<dyn VirtualTileOperation>> {
	let action = def.hash_get_str("action").context("while parsing an action")?;

	(match action {
		"pbf_replace_properties" => pbf_replace_properties::PBFReplacePropertiesOperation::new(def),
		_ => bail!("operation '{action}' is unknown"),
	})
	.with_context(|| format!("while parsing action '{action}'"))
}
