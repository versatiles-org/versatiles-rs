use super::operations::{new_tile_composer_operation, TileComposerOperation};
use crate::utils::YamlWrapper;
use anyhow::{bail, ensure, Result};
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
		new_tile_composer_operation(name, yaml, self).await
	}
}

#[cfg(test)]
mod tests {
	use std::str::FromStr;

	use super::*;

	#[tokio::test]
	async fn test_from_yaml() -> Result<()> {
		let yaml_data = r#"
        operation1:
          action: "pbf_replace_properties"
        operation2:
          action: "pbf_mock"
        "#;
		let yaml = YamlWrapper::from_str(yaml_data)?;
		let lookup = TileComposerOperationLookup::from_yaml(yaml)?;

		assert!(lookup.operations.contains_key("operation1"));
		assert!(lookup.operations.contains_key("operation2"));
		Ok(())
	}

	#[tokio::test]
	async fn test_insert() -> Result<()> {
		let mut lookup = TileComposerOperationLookup::new();
		let yaml = YamlWrapper::from_str("action: \"pbf_replace_properties\"")?;
		lookup.insert("operation1".to_string(), yaml.clone())?;
		assert!(lookup.operations.contains_key("operation1"));

		let result = lookup.insert("operation1".to_string(), yaml);
		assert!(result.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn test_construct() -> Result<()> {
		let yaml_data = r#"
        operation1:
          action: "read"
          filename: "../testdata/berlin.mbtiles"
        operation2:
          action: "pbf_mock"
        "#;
		let yaml = YamlWrapper::from_str(yaml_data)?;
		let mut lookup = TileComposerOperationLookup::from_yaml(yaml)?;

		let op = lookup.construct("operation1").await?;
		assert_eq!(
			format!("{:?}", op),
			"ReadOperation { name: \"operation1\", filename: \"../testdata/berlin.mbtiles\" }"
		);

		let result = lookup.construct("non_existing").await;
		assert!(result.is_err());
		Ok(())
	}
}
