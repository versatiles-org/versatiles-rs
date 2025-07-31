use crate::{
	PipelineFactory,
	helpers::read_csv_file,
	operations::vector::traits::{RunnerTrait, build_transform},
	traits::{OperationFactoryTrait, OperationTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use log::warn;
use std::collections::{HashMap, HashSet};
use versatiles_core::tilejson::TileJSON;
use versatiles_geometry::{GeoProperties, vector_tile::VectorTile};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Updates properties of vector tile features using data from an external source (e.g., CSV file). Matches features based on an ID field.
struct Args {
	/// Path to the data source file, e.g., `data_source_path="data.csv"`.
	data_source_path: String,

	/// Name of the vector layer to update.
	layer_name: String,

	/// ID field name in the vector layer.
	id_field_tiles: String,

	/// ID field name in the data source.
	id_field_data: String,

	/// If set, old properties will be deleted before new ones are added.
	replace_properties: Option<bool>,

	/// If set, removes all features (in the layer) that do not match.
	remove_non_matching: Option<bool>,

	/// If set, includes the ID field in the updated properties.
	include_id: Option<bool>,
}

#[derive(Debug)]
struct Runner {
	/// Parsed CLI / VPL arguments (layer name, key fields, flags …).
	args: Args,
	/// Lookup table keyed by **feature‑ID** (`id_field_data`) holding the
	/// new attribute sets parsed from the CSV.
	properties_map: HashMap<String, GeoProperties>,
}

impl Runner {
	pub fn from_args(args: Args, data: Vec<GeoProperties>) -> Result<Self> {
		// Convert each CSV row into a GeoProperties map.
		// Transform Vec<GeoProperties> into HashMap keyed by the data‑ID column.
		let properties_map = data
			.into_iter()
			.map(|mut properties| {
				let key = properties
					.get(&args.id_field_data)
					.ok_or_else(|| anyhow!("Key '{}' not found in CSV data", args.id_field_data))
					.with_context(|| {
						format!(
							"Failed to find key '{}' in the CSV data row: {properties:?}",
							args.id_field_data
						)
					})?
					.to_string();
				if !args.include_id.unwrap_or(false) {
					properties.remove(&args.id_field_data)
				}
				Ok((key, properties))
			})
			.collect::<Result<HashMap<String, GeoProperties>>>()
			.context("Failed to build properties map from CSV data")?;

		Ok(Self { args, properties_map })
	}
}

impl RunnerTrait for Runner {
	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		if let Some(layer) = tilejson.vector_layers.0.get_mut(&self.args.layer_name) {
			if self.args.replace_properties.unwrap_or(false) {
				layer.fields.clear();
			}

			let mut all_keys = HashSet::<String>::new();
			for prop in self.properties_map.values() {
				for (k, _) in prop.iter() {
					all_keys.insert(k.clone());
				}
			}
			for key in all_keys.into_iter() {
				layer.fields.insert(key, "automatically added field".to_string());
			}
		}
	}
	fn run(&self, mut tile: VectorTile) -> Result<VectorTile> {
		let layer_name = &self.args.layer_name;

		// Iterate over all layers in the tile and *only* touch the requested one.
		// Other layers pass through unchanged.
		let layer = tile.find_layer_mut(layer_name);
		if layer.is_none() {
			return Ok(tile);
		}

		layer.unwrap().filter_map_properties(|mut prop| {
			// For every feature grab its identifier; if absent, log a warning
			// and keep the feature unchanged.
			if let Some(id) = prop.get(&self.args.id_field_tiles) {
				// Look up the ID in our CSV‑derived map.  When found, merge or replace
				// the properties according to the flags.

				if let Some(new_prop) = self.properties_map.get(&id.to_string()) {
					if self.args.replace_properties.unwrap_or(false) {
						prop = new_prop.clone();
					} else {
						prop.update(new_prop);
					}
				} else {
					// Optionally drop features that failed the lookup.
					if self.args.remove_non_matching.unwrap_or(false) {
						return None;
					}
					warn!("id \"{id}\" not found in data source");
				}
			} else {
				warn!("id field \"{}\" not found", &self.args.id_field_tiles);
			}
			Some(prop)
		})?;

		Ok(tile)
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"vector_update_properties"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		// Load the CSV file referenced in the VPL.
		let data = read_csv_file(&factory.resolve_path(&args.data_source_path))
			.await
			.with_context(|| format!("Failed to read CSV file from '{}'", args.data_source_path))?;

		build_transform::<Runner>(source, Runner::from_args(args, data)?).await
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use pretty_assertions::assert_eq;
	use std::{fs::File, io::Write, vec};
	use versatiles_core::TileCoord3;
	use versatiles_geometry::{GeoFeature, GeoProperties, GeoValue, Geometry, vector_tile::VectorTileLayer};

	fn create_sample_vector_tile() -> VectorTile {
		let mut feature = GeoFeature::new(Geometry::new_example());
		feature.properties = GeoProperties::from(vec![
			("id", GeoValue::from("feature_1")),
			("property1", GeoValue::from("value1")),
		]);
		let layer = VectorTileLayer::from_features(String::from("test_layer"), vec![feature], 4096, 1).unwrap();
		VectorTile::new(vec![layer])
	}

	#[tokio::test]
	async fn test_runner_run() {
		let properties_map = HashMap::from([(
			"feature_1".to_string(),
			GeoProperties::from(vec![("property2", GeoValue::from("new_value"))]),
		)]);

		let runner = Runner {
			args: Args {
				data_source_path: "data.csv".to_string(),
				id_field_tiles: "id".to_string(),
				id_field_data: "id".to_string(),
				layer_name: "test_layer".to_string(),
				replace_properties: None,
				remove_non_matching: None,
				include_id: None,
			},
			properties_map,
		};

		let tile0 = create_sample_vector_tile();
		let tile1 = runner.run(tile0).unwrap();

		let properties = tile1.layers[0].features[0].decode_properties(&tile1.layers[0]).unwrap();

		assert_eq!(properties.get("property2").unwrap(), &GeoValue::from("new_value"));
	}

	#[test]
	fn test_args_from_vpl_node() {
		let vpl_node = VPLNode::from_str(
			r##"vector_update_properties data_source_path="data.csv" id_field_tiles=id id_field_data=id layer_name=test_layer replace_properties=true include_id=true"##,
		)
		.unwrap();

		let args = Args::from_vpl_node(&vpl_node).unwrap();
		assert_eq!(args.data_source_path, "data.csv");
		assert_eq!(args.id_field_tiles, "id");
		assert_eq!(args.id_field_data, "id");
		assert_eq!(args.replace_properties, Some(true));
		assert_eq!(args.include_id, Some(true));
		assert_eq!(args.layer_name, "test_layer");
		assert_eq!(args.remove_non_matching, None);
	}

	async fn run_test(replace_properties: bool, include_id: bool) -> Result<(String, String)> {
		// ── prepare tiny CSV on disk ────────────────────────────────
		let temp_file = NamedTempFile::new("test.csv")?;
		let mut file = File::create(&temp_file)?;
		writeln!(&mut file, "data_id,value\n1,test")?;

		// ── build pipeline ─────────────────────────────────────────
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(
				&[
					"from_debug |",
					"vector_update_properties",
					&format!(
						"data_source_path=\"{}\"",
						temp_file.to_str().unwrap().replace('\\', "\\\\")
					),
					"id_field_tiles=index",
					"id_field_data=data_id",
					"layer_name=debug_y",
					&format!("replace_properties={replace_properties}"),
					&format!("include_id={include_id}"),
				]
				.join(" "),
			)
			.await?;

		// ── extract a single feature for inspection ────────────────
		let blob = operation
			.get_tile_data(&TileCoord3::new(1000, 100, 10)?)
			.await?
			.unwrap();
		let tile = VectorTile::from_blob(&blob)?;
		let layer = tile.find_layer("debug_y").unwrap();

		// ── stringify for easy substring assertions ────────────────
		let prop_str = format!("{:?}", layer.features[1].decode_properties(layer)?);

		let fields = operation
			.tilejson()
			.vector_layers
			.find("debug_y")
			.unwrap()
			.fields
			.iter()
			.map(|(k, v)| format!("{k}: {v}"))
			.collect::<Vec<_>>()
			.join("\n");

		Ok((prop_str, fields))
	}

	#[tokio::test]
	async fn test_run_normal() {
		let (props, json) = run_test(false, false).await.unwrap();
		assert_eq!(
			props,
			"{\"char\": String(\":\"), \"index\": UInt(1), \"value\": String(\"test\"), \"x\": Float(132.7017)}"
		);
		assert_eq!(
			json.split('\n').collect::<Vec<_>>(),
			[
				"char: which character",
				"index: index of char",
				"value: automatically added field",
				"x: position",
			]
		);
	}

	#[tokio::test]
	async fn test_run_add_index() {
		let (props, json) = run_test(false, true).await.unwrap();
		assert_eq!(
			props,
			"{\"char\": String(\":\"), \"data_id\": UInt(1), \"index\": UInt(1), \"value\": String(\"test\"), \"x\": Float(132.7017)}"
		);
		assert_eq!(
			json.split('\n').collect::<Vec<_>>(),
			[
				"char: which character",
				"data_id: automatically added field",
				"index: index of char",
				"value: automatically added field",
				"x: position",
			]
		);
	}

	#[tokio::test]
	async fn test_run_replace() {
		let (props, json) = run_test(true, false).await.unwrap();
		assert_eq!(props, "{\"value\": String(\"test\")}");
		assert_eq!(
			json.split('\n').collect::<Vec<_>>(),
			["value: automatically added field",]
		);
	}

	#[tokio::test]
	async fn test_run_replace_and_include_index() {
		let (props, json) = run_test(true, true).await.unwrap();
		assert_eq!(props, "{\"data_id\": UInt(1), \"value\": String(\"test\")}");
		assert_eq!(
			json.split('\n').collect::<Vec<_>>(),
			["data_id: automatically added field", "value: automatically added field",]
		);
	}
}
