use super::{TileComposerOperation, TileComposerOperationLookup};
use crate::{
	container::{composer::utils::read_csv_file, TilesReaderParameters, TilesStream},
	geometry::{vector_tile::VectorTile, GeoProperties},
	types::Blob,
	utils::{decompress, YamlWrapper},
};
use anyhow::{anyhow, ensure, Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};
use versatiles_core::types::{TileBBox, TileCompression, TileCoord3, TileFormat};
use versatiles_derive::YamlParser;

#[derive(Debug, YamlParser)]
struct Config {
	data_source_path: String,
	id_field_tiles: String,
	id_field_values: String,
	replace_properties: bool,
	remove_empty_properties: bool,
	also_save_id: bool,
}

/// The `PBFUpdatePropertiesOperation` struct represents an operation that replaces properties in PBF tiles
/// based on a mapping provided in a CSV file.
pub struct Runner {
	config: Config,
	properties_map: HashMap<String, GeoProperties>,
}

impl Runner {
	fn new(yaml: &YamlWrapper) -> Result<Self>
	where
		Self: Sized,
	{
		let config = Config::from_yaml(yaml)?;

		let data = read_csv_file(Path::new(&config.data_source_path))
			.with_context(|| format!("Failed to read CSV file from '{}'", config.data_source_path))?;

		let properties_map = data
			.into_iter()
			.map(|mut properties| {
				let key = properties
					.get(&config.id_field_values)
					.ok_or_else(|| anyhow!("Key '{}' not found in CSV data", config.id_field_values))
					.with_context(|| {
						format!(
							"Failed to find key '{}' in the CSV data row: {properties:?}",
							config.id_field_values
						)
					})?
					.to_string();
				if !config.also_save_id {
					properties.remove(&config.id_field_values)
				}
				Ok((key, properties))
			})
			.collect::<Result<HashMap<String, GeoProperties>>>()
			.context("Failed to build properties map from CSV data")?;

		Ok(Runner {
			config,
			properties_map,
		})
	}
	fn run(&self, blob: Blob) -> Result<Option<Blob>> {
		let mut tile =
			VectorTile::from_blob(&blob).context("Failed to create VectorTile from Blob")?;

		for layer in tile.layers.iter_mut() {
			layer.map_properties(|properties| {
				if let Some(mut prop) = properties {
					if let Some(id) = prop.get(&self.config.id_field_tiles) {
						if let Some(new_prop) = self.properties_map.get(&id.to_string()) {
							if self.config.replace_properties {
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

			if self.config.remove_empty_properties {
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

impl Debug for Runner {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Runner")
			.field("config", &self.config)
			.field(
				"properties_map",
				&std::collections::BTreeMap::from_iter(self.properties_map.iter()),
			)
			.finish()
	}
}

#[allow(dead_code)]
pub struct PBFUpdatePropertiesOperation {
	runner: Arc<Runner>,
	input: Box<dyn TileComposerOperation>,
	name: String,
	parameters: TilesReaderParameters,
	input_compression: TileCompression,
}

#[async_trait]
impl TileComposerOperation for PBFUpdatePropertiesOperation {
	/// Creates a new `PBFUpdatePropertiesOperation` from the provided YAML configuration.
	///
	/// # Arguments
	///
	/// * `yaml` - A reference to a `YamlWrapper` containing the configuration.
	///
	/// # Returns
	///
	/// * `Result<PBFUpdatePropertiesOperation>` - The constructed operation or an error if the configuration is invalid.
	async fn new(
		name: &str,
		yaml: YamlWrapper,
		lookup: &mut TileComposerOperationLookup,
	) -> Result<Self>
	where
		Self: Sized,
	{
		let runner = Arc::new(Runner::new(&yaml)?);

		let input_name = yaml.hash_get_str("input")?;
		let input = lookup.construct(input_name).await?;

		let mut parameters = input.get_parameters().clone();
		ensure!(
			parameters.tile_format == TileFormat::PBF,
			"operation '{name}' needs vector tiles (PBF) from '{input_name}'",
		);

		let input_compression = parameters.tile_compression;
		parameters.tile_compression = TileCompression::Uncompressed;

		Ok(PBFUpdatePropertiesOperation {
			runner,
			input,
			input_compression,
			name: name.to_string(),
			parameters,
		})
	}

	fn get_name(&self) -> &str {
		&self.name
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TilesStream {
		let compression = self.input_compression;
		let runner = self.runner.clone();

		self
			.input
			.get_bbox_tile_stream(bbox)
			.await
			.map(move |(coord, blob)| {
				let runner = runner.clone();
				tokio::spawn(async move {
					let blob = decompress(blob, &compression).unwrap();
					let blob = runner.run(blob).unwrap();
					blob.map(|inner| (coord, inner))
				})
			})
			.buffer_unordered(num_cpus::get())
			.filter_map(|e| futures::future::ready(e.unwrap()))
			.boxed()
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	async fn get_meta(&self) -> Result<Option<Blob>> {
		self.input.get_meta().await
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let blob = self.input.get_tile_data(coord).await?;
		if let Some(blob) = blob {
			self.runner.run(decompress(blob, &self.input_compression)?)
		} else {
			Ok(None)
		}
	}
}

impl Debug for PBFUpdatePropertiesOperation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PBFUpdatePropertiesOperation")
			.field("name", &self.name)
			.field("input", &self.input.get_name())
			.field("input_compression", &self.input_compression)
			.field("parameters", &self.parameters)
			.field("runner", &self.runner)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::{TileComposerReader, TilesReader};

	async fn test(
		parameters: (&str, &str, &[(&str, bool)]),
		debug_operation: &str,
		debug_result: &str,
	) -> Result<()> {
		let flags = parameters
			.2
			.iter()
			.map(|(key, val)| format!("    {key}: {}", if *val { "true" } else { "false" }))
			.collect::<Vec<String>>()
			.join("\n");

		let yaml = vec![
			"operations:",
			"  source:",
			"    action: pbf_mock",
			"  update_values:",
			"    action: pbf_update_properties",
			"    input: source",
			"    data_source_path: ../testdata/cities.csv",
			&format!("    id_field_tiles: {}", parameters.0),
			&format!("    id_field_values: {}", parameters.1),
			&flags,
			"output: update_values",
		]
		.join("\n");

		let mut reader = TileComposerReader::open_str(&yaml).await?;

		assert_eq!(cleanup(format!("{:?}", reader.output)), debug_operation);

		let blob = reader
			.get_tile_data(&TileCoord3::new(0, 0, 0)?)
			.await?
			.unwrap();

		let layers = VectorTile::from_blob(&blob)?.layers;
		assert_eq!(layers.len(), 1);
		let tile = layers[0].to_features()?;

		assert_eq!(cleanup(format!("{tile:?}")), debug_result);

		fn cleanup(text: String) -> String {
			text
			.replace(
				"PBFUpdatePropertiesOperation { name: \"update_values\", input: \"source\", input_compression: Uncompressed, parameters: TilesReaderParameters { bbox_pyramid: [0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64), 4: [0,0,15,15] (256), 5: [0,0,31,31] (1024), 6: [0,0,63,63] (4096), 7: [0,0,127,127] (16384), 8: [0,0,255,255] (65536), 9: [0,0,511,511] (262144), 10: [0,0,1023,1023] (1048576), 11: [0,0,2047,2047] (4194304), 12: [0,0,4095,4095] (16777216), 13: [0,0,8191,8191] (67108864), 14: [0,0,16383,16383] (268435456), 15: [0,0,32767,32767] (1073741824), 16: [0,0,65535,65535] (4294967296), 17: [0,0,131071,131071] (17179869184), 18: [0,0,262143,262143] (68719476736), 19: [0,0,524287,524287] (274877906944), 20: [0,0,1048575,1048575] (1099511627776), 21: [0,0,2097151,2097151] (4398046511104), 22: [0,0,4194303,4194303] (17592186044416), 23: [0,0,8388607,8388607] (70368744177664), 24: [0,0,16777215,16777215] (281474976710656), 25: [0,0,33554431,33554431] (1125899906842624), 26: [0,0,67108863,67108863] (4503599627370496), 27: [0,0,134217727,134217727] (18014398509481984), 28: [0,0,268435455,268435455] (72057594037927936), 29: [0,0,536870911,536870911] (288230376151711744), 30: [0,0,1073741823,1073741823] (1152921504606846976), 31: [0,0,2147483647,2147483647] (4611686018427387904)], tile_compression: Uncompressed, tile_format: PBF }, runner: Runner { config: Config { data_source_path: \"../testdata/cities.csv\",",
				"Intro…",
			)
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

	#[tokio::test]
	async fn test_new() -> Result<()> {
		test(
			("tile_id", "city_id", &[]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: false, remove_empty_properties: false, also_save_id: false }, properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]"
		).await
	}

	#[tokio::test]
	async fn test_unknown_key() {
		assert_eq!(
			test(("tile_id", "unknown_id", &[]), "", "")
				.await
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"Key 'unknown_id' not found in CSV data"
		);
	}

	#[tokio::test]
	async fn test_replace_properties() -> Result<()> {
		test(
			("tile_id", "city_id", &[("replace_properties", false)]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: false, remove_empty_properties: false, also_save_id: false }, properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]"
		).await?;
		test(
			("tile_id", "city_id", &[("replace_properties", true)]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: true, remove_empty_properties: false, also_save_id: false }, properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({…Berlin…}) }, Feature { id: None, …geometry… properties: None }]"
		).await
	}

	#[tokio::test]
	async fn test_remove_empty_properties() -> Result<()> {
		test(
			("tile_id", "city_id", &[("remove_empty_properties", false)]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: false, remove_empty_properties: false, also_save_id: false }, properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]"
		).await?;
		test(
			("tile_id", "city_id", &[("remove_empty_properties", true)]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: false, remove_empty_properties: true, also_save_id: false }, properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }]"
		).await
	}

	#[tokio::test]
	async fn test_also_save_id() -> Result<()> {
		test(
			("tile_id", "city_id", &[("also_save_id", false)]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: false, remove_empty_properties: false, also_save_id: false }, properties_map: {\"1\": {…Berlin…}, \"2\": {…Kyiv…}, \"3\": {…Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({…Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]"
		).await?;
		test(
			("tile_id", "city_id", &[("also_save_id", true)]),
			"Intro… id_field_tiles: \"tile_id\", id_field_values: \"city_id\", replace_properties: false, remove_empty_properties: false, also_save_id: true }, properties_map: {\"1\": {\"city_id\": UInt(1), …Berlin…}, \"2\": {\"city_id\": UInt(2), …Kyiv…}, \"3\": {\"city_id\": UInt(3), …Plovdiv…}} } }",
			"[Feature { id: None, …geometry… properties: Some({\"city_id\": UInt(1), …Berlin…, \"tile_id\": UInt(1), \"tile_name\": String(\"Bärlin\")}) }, Feature { id: None, …geometry… properties: None }]"
		).await
	}
}
