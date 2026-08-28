use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use versatiles_container::TileSource;
use versatiles_core::TileJSON;
use versatiles_derive::context;
use versatiles_geometry::{geo::GeoProperties, vector_tile::VectorTile};

use crate::helpers::location::FilePath;
use crate::{
	PipelineFactory,
	helpers::{CsvReader, SeparatorChar},
	operations::transform::{AsTileTransform, TransformOp, VectorTransform},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Joins tabular data onto vector features, matching on an id column.
///
/// Each row of a CSV or TSV file is matched to the features whose
/// `id_field_tiles` property equals the row's `id_field_data` column, and the
/// row's remaining columns become properties on those features. This is how a
/// published statistics table is attached to the geometry it refers to — see
/// `from_grid` and `from_h3` for generating that geometry.
struct Args {
	/// Path to the CSV or TSV file, which must have a header row.
	data_source_path: FilePath,

	/// Name of the layer whose features are updated.
	#[vpl(layer_of_source)]
	layer_name: String,

	/// Feature property holding the id to match on.
	#[vpl(field_of_source)]
	id_field_tiles: String,

	/// Column in the data file holding the id to match on.
	#[vpl(field_of = "data_source_path")]
	id_field_data: String,

	/// Whether to replace a feature's properties instead of merging. Defaults to `false`.
	#[vpl(default = "false")]
	replace_properties: Option<bool>,

	/// Whether to drop features that have no matching row. Defaults to `false`.
	#[vpl(default = "false")]
	remove_non_matching: Option<bool>,

	/// Whether to keep the id column among the written properties. Defaults to `false`.
	#[vpl(default = "false")]
	include_id: Option<bool>,

	/// Character separating a row's fields. Defaults to `,` for `.csv` and a tab for `.tsv`.
	field_separator: Option<SeparatorChar>,

	/// Decimal separator for parsing numbers, so `,` reads `1.234,56`. Defaults to `.`.
	#[vpl(default = ".")]
	decimal_separator: Option<SeparatorChar>,
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
	#[context("Failed to build vector update properties runner")]
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
					properties.remove(&args.id_field_data);
				}
				Ok((key, properties))
			})
			.collect::<Result<HashMap<String, GeoProperties>>>()
			.context("Failed to build properties map from CSV data")?;

		Ok(Self { args, properties_map })
	}
}

impl VectorTransform for Runner {
	const TAG: &'static str = "vector_update_properties";

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
			for key in all_keys {
				layer.fields.insert(key, "automatically added field".to_string());
			}
		}
	}

	#[context("Failed to run vector update properties")]
	fn run(&self, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		let layer_name = &self.args.layer_name;

		// Iterate over all layers in the tile and *only* touch the requested one.
		// Other layers pass through unchanged.
		let layer = tile.find_layer_mut(layer_name);
		if layer.is_none() {
			return Ok(Some(tile));
		}

		layer
			.expect("early-returned above if layer is None")
			.filter_map_properties(|mut prop| {
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
						log::info!("id \"{id}\" not found in data source");
					}
				} else {
					log::warn!("id field \"{}\" not found", self.args.id_field_tiles);
				}
				Some(prop)
			})?;

		Ok(Some(tile))
	}
}

impl Runner {
	#[context("Building vector_update_properties operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<AsTileTransform<Runner>>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		let path = factory
			.resolve_location(&args.data_source_path.to_location())?
			.to_path_buf()?;
		let mut csv_reader = CsvReader::new(&path, factory.runtime()).with_string_field(&args.id_field_data);

		if let Some(sep) = args.field_separator {
			csv_reader = csv_reader.with_field_separator(sep.char());
		}
		if let Some(sep) = args.decimal_separator {
			csv_reader = csv_reader.with_decimal_separator(sep.char());
		}

		// Load the CSV file referenced in the VPL.
		let data = csv_reader
			.read()
			.await
			.with_context(|| format!("Failed to read CSV file from '{}'", args.data_source_path))?;

		Ok(TransformOp::new(
			source,
			AsTileTransform(Runner::from_args(args, data)?),
			factory.runtime(),
		))
	}
}

crate::operations::macros::define_transform_factory!("vector_update_properties", Args, Runner, requires: Vector);

/// Parses a separator string into a single character.
/// Supports escape sequences like "\t" for tab.
// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use std::{fs::File, io::Write, vec};

	use assert_fs::NamedTempFile;
	use pretty_assertions::assert_eq;
	use versatiles_core::TileCoord;
	use versatiles_geometry::{
		geo::{GeoFeature, GeoProperties, GeoValue, example_geometry},
		vector_tile::VectorTileLayer,
	};

	use super::*;

	fn create_sample_vector_tile() -> VectorTile {
		let mut feature = GeoFeature::new(example_geometry());
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
				data_source_path: FilePath::try_from("data.csv").unwrap(),
				id_field_tiles: "id".to_string(),
				id_field_data: "id".to_string(),
				layer_name: "test_layer".to_string(),
				replace_properties: None,
				remove_non_matching: None,
				include_id: None,
				field_separator: None,
				decimal_separator: None,
			},
			properties_map,
		};

		let tile0 = create_sample_vector_tile();
		let tile1 = runner.run(tile0).unwrap().unwrap();

		let properties = tile1.layers[0].features[0].decode_properties(&tile1.layers[0]).unwrap();

		assert_eq!(properties.get("property2").unwrap(), &GeoValue::from("new_value"));
	}

	#[test]
	fn test_args_from_vpl_node() {
		let vpl_node = VPLNode::try_from_str(
			r#"vector_update_properties data_source_path="data.csv" id_field_tiles=id id_field_data=id layer_name=test_layer replace_properties=true include_id=true"#,
		)
		.unwrap();

		let args = Args::from_vpl_node(&vpl_node).unwrap();
		assert_eq!(args.data_source_path.as_path(), std::path::Path::new("data.csv"));
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
		let mut stream = operation
			.tile_stream(TileCoord::new(10, 1000, 100)?.to_tile_bbox())
			.await?;
		let tile = stream.next().await.unwrap().1.into_vector()?;
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
			"{\"char\": String(\":\"), \"data_id\": String(\"1\"), \"index\": UInt(1), \"value\": String(\"test\"), \"x\": Float(132.7017)}"
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
		assert_eq!(props, "{\"data_id\": String(\"1\"), \"value\": String(\"test\")}");
		assert_eq!(
			json.split('\n').collect::<Vec<_>>(),
			["data_id: automatically added field", "value: automatically added field",]
		);
	}

	#[tokio::test]
	async fn output_tiles_pass_mvt_validation() -> Result<()> {
		use versatiles_core::TileBBox;

		use crate::helpers::assert_tiles_valid;
		let temp_file = NamedTempFile::new("test.csv")?;
		let mut file = File::create(&temp_file)?;
		writeln!(&mut file, "data_id,value\n1,test")?;

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				&[
					"from_debug |",
					"vector_update_properties",
					&format!(
						r#"data_source_path="{}""#,
						temp_file.to_str().unwrap().replace('\\', "\\\\")
					),
					"id_field_tiles=index",
					"id_field_data=data_id",
					"layer_name=debug_y",
				]
				.join(" "),
			)
			.await?;
		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}
}
