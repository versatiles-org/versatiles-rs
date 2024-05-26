use super::TileComposerOperation;
use crate::{
	container::composer::utils::read_csv_file,
	types::Blob,
	utils::{
		geometry::{vector_tile::VectorTile, GeoProperties},
		YamlWrapper,
	},
};
use anyhow::{anyhow, Context, Result};
use std::{
	collections::{BTreeMap, HashMap},
	fmt::Debug,
	path::Path,
};

/// The `PBFReplacePropertiesOperation` struct represents an operation that replaces properties in PBF tiles
/// based on a mapping provided in a CSV file.
pub struct PBFReplacePropertiesOperation {
	pub properties_map: HashMap<String, GeoProperties>,
	pub id_field_tiles: String,
	pub remove_empty_properties: bool,
	pub replace_properties: bool,
}

impl TileComposerOperation for PBFReplacePropertiesOperation {
	/// Creates a new `PBFReplacePropertiesOperation` from the provided YAML configuration.
	///
	/// # Arguments
	///
	/// * `yaml` - A reference to a `YamlWrapper` containing the configuration.
	///
	/// # Returns
	///
	/// * `Result<PBFReplacePropertiesOperation>` - The constructed operation or an error if the configuration is invalid.
	fn new(yaml: &YamlWrapper) -> Result<PBFReplacePropertiesOperation>
	where
		Self: Sized,
	{
		let data_source_path = yaml
			.hash_get_string("data_source_path")
			.context("Failed to get 'data_source_path' from YAML configuration")?;
		let id_field_tiles = yaml
			.hash_get_string("id_field_tiles")
			.context("Failed to get 'id_field_tiles' from YAML configuration")?;
		let id_field_values = yaml
			.hash_get_string("id_field_values")
			.context("Failed to get 'id_field_values' from YAML configuration")?;
		let replace_properties = yaml.hash_get_bool("replace_properties").unwrap_or(false);
		let remove_empty_properties = yaml
			.hash_get_bool("remove_empty_properties")
			.unwrap_or(false);
		let also_save_id = yaml.hash_get_bool("also_save_id").unwrap_or(false);

		let data = read_csv_file(Path::new(&data_source_path))
			.with_context(|| format!("Failed to read CSV file from '{data_source_path}'"))?;

		let properties_map = data
			.into_iter()
			.map(|mut properties| {
				let key =
					properties
						.get(&id_field_values)
						.ok_or_else(|| anyhow!("Key '{id_field_values}' not found in CSV data"))
						.with_context(|| {
							format!("Failed to find key '{id_field_values}' in the CSV data row: {properties:?}")
						})?
						.to_string();
				if !also_save_id {
					properties.remove(&id_field_values)
				}
				Ok((key, properties))
			})
			.collect::<Result<HashMap<String, GeoProperties>>>()
			.context("Failed to build properties map from CSV data")?;

		Ok(PBFReplacePropertiesOperation {
			id_field_tiles,
			properties_map,
			remove_empty_properties,
			replace_properties,
		})
	}

	/// Runs the operation on the provided blob, replacing properties in the PBF tile.
	///
	/// # Arguments
	///
	/// * `blob` - A reference to the `Blob` to be processed.
	///
	/// # Returns
	///
	/// * `Result<Option<Blob>>` - The processed blob or an error if processing failed.
	fn run(&self, blob: &Blob) -> Result<Option<Blob>> {
		let mut tile =
			VectorTile::from_blob(blob).context("Failed to create VectorTile from Blob")?;

		for layer in tile.layers.iter_mut() {
			layer.map_properties(|properties| {
				if let Some(mut prop) = properties {
					if let Some(id) = prop.get(&self.id_field_tiles) {
						if let Some(new_prop) = self.properties_map.get(&id.to_string()) {
							if self.replace_properties {
								prop = new_prop.clone();
							} else {
								prop.update(new_prop.clone());
							}
							return Some(prop);
						}
					}
				}
				None
			})?;

			if self.remove_empty_properties {
				layer.retain_features(|feature| !feature.tag_ids.is_empty());
			}
		}

		Ok(Some(
			tile
				.to_blob()
				.context("Failed to convert VectorTile to Blob")?,
		))
	}
}

impl Debug for PBFReplacePropertiesOperation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PBFReplacePropertiesOperation")
			.field(
				"properties_map",
				&BTreeMap::from_iter(self.properties_map.iter()),
			)
			.field("id_field_tiles", &self.id_field_tiles)
			.field("remove_empty_properties", &self.remove_empty_properties)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::utils::geometry::{vector_tile::VectorTileLayer, Feature, GeoValue, Geometry};
	use std::str::FromStr;

	fn test(
		parameters: (&str, &str, &[(&str, bool)]), debug_operation: &str, debug_result: &str,
	) -> Result<()> {
		let mut yaml = vec![
			"data_source_path: \"testdata/cities.csv\"".to_string(),
			format!("id_field_tiles: \"{}\"", parameters.0),
			format!("id_field_values: \"{}\"", parameters.1),
		];
		for (key, val) in parameters.2.iter() {
			yaml.push(format!("{key}: {}", if *val { "true" } else { "false" }));
		}
		let yaml = yaml.join("\n");
		let operation = PBFReplacePropertiesOperation::new(&YamlWrapper::from_str(&yaml)?)?;

		assert_eq!(cleanup(format!("{operation:?}")), debug_operation);

		let tile_blob = (VectorTile {
			layers: vec![VectorTileLayer::from_features(
				"test_layer".to_string(),
				vec![(1, "Bärlin"), (4, "Madrid")]
					.into_iter()
					.map(|(id, name)| Feature {
						id: None,
						geometry: Geometry::new_example(),
						properties: Some(GeoProperties::from(vec![
							("tile_id", GeoValue::from(id)),
							("tile_name", GeoValue::from(name)),
						])),
					})
					.collect(),
				4096,
				2,
			)?],
		})
		.to_blob()?;

		let result = operation.run(&tile_blob)?.map(|blob| {
			VectorTile::from_blob(&blob)
				.unwrap()
				.layers
				.iter()
				.map(|layer| layer.to_features())
				.collect::<Result<Vec<Vec<Feature>>>>()
				.unwrap()
		});

		assert_eq!(cleanup(format!("{result:?}")), debug_result);

		fn cleanup(text: String) -> String {
			text
				.replace(
					"\"city_name\": String(\"Berlin\"), \"city_population\": UInt(3755251)",
					"…Berlin…",
				)
				.replace(
					"\"city_name\": String(\"Kyiv\"), \"city_population\": UInt(2952301)",
					"…Kyiv…",
				)
				.replace(
					"\"city_name\": String(\"Plovdiv\"), \"city_population\": UInt(346893)",
					"…Plovdiv…",
				)
				.replace(
					"geometry: MultiPolygon([[[[0.0, 0.0], [5.0, 0.0], [3.0, 4.0], [0.0, 0.0]], [[2.0, 1.0], [3.0, 2.0], [3.0, 1.0], [2.0, 1.0]]], [[[6.0, 0.0], [9.0, 0.0], [9.0, 4.0], [6.0, 4.0], [6.0, 0.0]], [[7.0, 1.0], [7.0, 3.0], [8.0, 3.0], [8.0, 1.0], [7.0, 1.0]]]]),", 
					"…geometry…"
				)
		}

		Ok(())
	}

	#[test]
	fn test_new() -> Result<()> {
		test(
			("tile_id", "city_id", &[]),
			 "PBFReplacePropertiesOperation { properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: false }",
			 "Some([[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]])"
		)
	}

	#[test]
	fn test_unknown_key() {
		assert_eq!(
			test(("tile_id", "unknown_id", &[]), "", "")
				.unwrap_err()
				.root_cause()
				.to_string(),
			"Key 'unknown_id' not found in CSV data"
		);
	}

	#[test]
	fn test_replace_properties() -> Result<()> {
		test(
			("tile_id", "city_id", &[("replace_properties", false)]), 
			"PBFReplacePropertiesOperation { properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: false }",
			"Some([[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]])"
		)?;
		test(
			("tile_id", "city_id", &[("replace_properties", true)]), 
			"PBFReplacePropertiesOperation { properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: false }",
			"Some([[Feature { id: None, …geometry… properties: Some({…Berlin…}) }, Feature { id: None, …geometry… properties: None }]])"
		)
	}

	#[test]
	fn test_remove_empty_properties() -> Result<()> {
		test(
			("tile_id", "city_id", &[("remove_empty_properties", false)]), 
			"PBFReplacePropertiesOperation { properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: false }",
			"Some([[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]])"
		)?;
		test(
			("tile_id", "city_id", &[("remove_empty_properties", true)]), 
			"PBFReplacePropertiesOperation { properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: true }",
			"Some([[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }]])"
		)
	}

	#[test]
	fn test_also_save_id() -> Result<()> {
		test(
			("tile_id", "city_id", &[("also_save_id", false)]), 
			"PBFReplacePropertiesOperation { properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: false }",
			"Some([[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]])"
		)?;
		test(
			("tile_id", "city_id", &[("also_save_id", true)]), 
			"PBFReplacePropertiesOperation { properties_map: {\"1\": {\"city_id\": UInt(1), …Berlin…}, \"2\": {\"city_id\": UInt(2), …Kyiv…}, \"3\": {\"city_id\": UInt(3), …Plovdiv…}}, id_field_tiles: \"tile_id\", remove_empty_properties: false }",
			"Some([[Feature { id: None, …geometry… properties: Some({\"city_id\": UInt(1), …Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]])"
		)
	}
}
