/*!
The `composer` module provides functionality for reading, processing, and composing tiles from multiple sources.
*/

mod operations;
mod output;
mod utils;

mod reader;

use crate::utils::YamlWrapper;
use anyhow::{bail, ensure, Result};
use operations::{new_tile_composer_operation, TileComposerOperation};
pub use reader::TileComposerReader;
use std::collections::HashMap;

pub struct TileComposerOperationLookup {
	operations: HashMap<String, YamlWrapper>,
}

impl TileComposerOperationLookup {
	fn new() -> Self {
		Self {
			operations: HashMap::new(),
		}
	}

	pub fn from_yaml(yaml: YamlWrapper) -> Result<Self> {
		ensure!(yaml.is_hash(), "must be an object");
		let mut lookup = Self::new();
		for (name, entry) in yaml.hash_get_as_vec()?.into_iter() {
			lookup.insert(name, entry)?;
		}
		Ok(lookup)
	}

	fn insert(&mut self, name: String, yaml: YamlWrapper) -> Result<()> {
		if self.operations.contains_key(&name) {
			bail!("operation '{name}' already exists")
		}
		self.operations.insert(name, yaml);
		Ok(())
	}

	pub async fn construct(&mut self, name: &str) -> Result<Box<dyn TileComposerOperation>> {
		if !self.operations.contains_key(name) {
			bail!("operation '{name}' not found")
		}
		let yaml = self.operations.remove(name).unwrap();
		Ok(new_tile_composer_operation(name, yaml, self).await?)
	}
}
