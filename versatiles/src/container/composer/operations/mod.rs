mod pbf_update_properties;

use crate::{types::Blob, utils::YamlWrapper};
use anyhow::{bail, Context, Result};
use std::fmt::Debug;

/// The `TileComposerOperation` trait defines the interface for operations that can be applied
/// to tiles in the Composer module.
pub trait TileComposerOperation: Debug + Send + Sync {
	/// Creates a new instance of the operation from the provided YAML configuration.
	///
	/// # Arguments
	///
	/// * `def` - A reference to a `YamlWrapper` containing the configuration.
	///
	/// # Returns
	///
	/// * `Result<Self>` - The constructed operation or an error if the configuration is invalid.
	fn new(def: &YamlWrapper) -> Result<Self>
	where
		Self: Sized;

	/// Runs the operation on the provided blob.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to be processed.
	///
	/// # Returns
	///
	/// * `Result<Option<Blob>>` - The processed blob or an error if processing failed.
	fn run(&self, blob: &Blob) -> Result<Option<Blob>>;
}

/// Creates a new tile composer operation based on the provided YAML configuration.
///
/// # Arguments
///
/// * `def` - A reference to a `YamlWrapper` containing the configuration.
///
/// # Returns
///
/// * `Result<Box<dyn TileComposerOperation>>` - The constructed operation or an error if the configuration is invalid.
pub fn new_tile_composer_operation(def: &YamlWrapper) -> Result<Box<dyn TileComposerOperation>> {
	let action = def.hash_get_str("action").context("while parsing action")?;

	match action {
		"pbf_replace_properties" => Ok(Box::new(
			pbf_update_properties::PBFReplacePropertiesOperation::new(def)
				.with_context(|| format!("Failed parsing action '{action}'"))?,
		)),
		_ => bail!("operation '{action}' is unknown"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::str::FromStr;

	#[test]
	fn test_new_tile_composer_operation() -> Result<()> {
		let yaml_str = r#"
        action: "pbf_replace_properties"
        data_source_path: "../testdata/cities.csv"
        id_field_tiles: "tile_id"
        id_field_values: "city_id"
        replace_properties: true
        remove_empty_properties: false
        "#;
		let yaml = YamlWrapper::from_str(yaml_str)?;
		new_tile_composer_operation(&yaml)?;
		Ok(())
	}

	#[test]
	fn test_unknown_action() {
		let yaml_str = r#"
        action: "unknown_action"
        "#;
		let yaml = YamlWrapper::from_str(yaml_str).unwrap();
		let result = new_tile_composer_operation(&yaml);
		assert!(result.is_err());
		assert_eq!(
			result.unwrap_err().to_string(),
			"operation 'unknown_action' is unknown"
		);
	}

	#[test]
	fn test_missing_action() {
		let yaml_str = r#"
        data_source_path: "../testdata/cities.csv"
        id_field_tiles: "tile_id"
        id_field_values: "city_id"
        replace_properties: true
        remove_empty_properties: false
        "#;
		let yaml = YamlWrapper::from_str(yaml_str).unwrap();
		let result = new_tile_composer_operation(&yaml);
		assert!(result.is_err());
		assert!(result
			.unwrap_err()
			.to_string()
			.contains("while parsing action"));
	}
}
