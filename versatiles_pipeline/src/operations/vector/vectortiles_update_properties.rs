use crate::{
	helpers::{pack_vector_tile, pack_vector_tile_stream, read_csv_file},
	traits::{OperationFactoryTrait, OperationTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
	PipelineFactory,
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use log::warn;
use std::{
	collections::{BTreeSet, HashMap},
	sync::Arc,
};
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::{vector_tile::VectorTile, GeoProperties};

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
	replace_properties: bool,

	/// If set, removes all features (in the layer) that do not match.
	remove_non_matching: bool,

	/// If set, includes the ID field in the updated properties.
	include_id: bool,
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
	pub fn run(&self, mut tile: VectorTile) -> Result<VectorTile> {
		let layer_name = &self.args.layer_name;

		// Iterate over all layers in the tile and *only* touch the requested one.
		// Other layers pass through unchanged.
		for layer in tile.layers.iter_mut() {
			if &layer.name != layer_name {
				continue;
			}

			layer.filter_map_properties(|mut prop| {
				// For every feature grab its identifier; if absent, log a warning
				// and keep the feature unchanged.
				if let Some(id) = prop.get(&self.args.id_field_tiles) {
					// Look up the ID in our CSV‑derived map.  When found, merge or replace
					// the properties according to the flags.
					if let Some(new_prop) = self.properties_map.get(&id.to_string()) {
						if self.args.replace_properties {
							prop = new_prop.clone();
						} else {
							prop.update(new_prop);
						}
					} else {
						// Optionally drop features that failed the lookup.
						if self.args.remove_non_matching {
							return None;
						}
						warn!("id \"{id}\" not found in data source");
					}
				} else {
					warn!("id field \"{}\" not found", &self.args.id_field_tiles);
				}
				Some(prop)
			})?;
		}

		Ok(tile)
	}
}

#[derive(Debug)]
struct Operation {
	/// Shared transformer that patches every vector tile.
	runner: Arc<Runner>,
	/// Output reader parameters (same as source but uncompressed).
	parameters: TilesReaderParameters,
	/// Upstream operation that delivers the *original* tiles.
	source: Box<dyn OperationTrait>,
	/// TileJSON after adding any new attribute keys discovered in the CSV.
	tilejson: TileJSON,
}

impl Operation {
	fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			// Parse the VPL node into strongly‑typed arguments.
			let data = read_csv_file(&factory.resolve_path(&args.data_source_path))
				.await
				.with_context(|| format!("Failed to read CSV file from '{}'", args.data_source_path))?;

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
					if !args.include_id {
						properties.remove(&args.id_field_data)
					}
					Ok((key, properties))
				})
				.collect::<Result<HashMap<String, GeoProperties>>>()
				.context("Failed to build properties map from CSV data")?;

			let mut parameters = source.get_parameters().clone();
			ensure!(parameters.tile_format == TileFormat::MVT, "source must be vector tiles");

			let mut tilejson = source.get_tilejson().clone();
			if let Some(layer) = tilejson.vector_layers.0.get_mut(&args.layer_name) {
				let mut all_keys = BTreeSet::<String>::new();
				for prop in properties_map.values() {
					for (k, _) in prop.iter() {
						if !prop.0.contains_key(k) {
							all_keys.insert(k.clone());
						}
					}
				}
				if args.replace_properties {
					layer.fields.clear();
				}
				for key in all_keys {
					layer.fields.insert(key, "automatically added field".to_string());
				}
			}

			let runner = Arc::new(Runner { args, properties_map });

			parameters.tile_compression = TileCompression::Uncompressed;
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				runner,
				parameters,
				source,
				tilejson,
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		pack_vector_tile(self.get_vector_data(coord).await, &self.parameters)
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_vector_tile_stream(self.get_vector_stream(bbox).await, &self.parameters)
	}

	async fn get_image_data(&self, _coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		bail!("this operation does not support image data");
	}

	async fn get_image_stream(&self, _bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		bail!("this operation does not support image data");
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		if let Some(tile) = self.source.get_vector_data(coord).await? {
			self.runner.run(tile).map(Some)
		} else {
			Ok(None)
		}
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		let runner = self.runner.clone();
		Ok(self
			.source
			.get_vector_stream(bbox)
			.await?
			.filter_map_item_parallel(move |tile| runner.run(tile).map(Some)))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"vectortiles_update_properties"
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
		Operation::build(vpl_node, source, factory).await
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use pretty_assertions::assert_eq;
	use std::{fs::File, io::Write};
	use versatiles_geometry::{vector_tile::VectorTileLayer, GeoFeature, GeoProperties, GeoValue, Geometry};

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
				replace_properties: false,
				remove_non_matching: false,
				include_id: false,
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
			r##"vectortiles_update_properties data_source_path="data.csv" id_field_tiles=id id_field_data=id layer_name=test_layer replace_properties=true include_id=true"##,
		)
		.unwrap();

		let args = Args::from_vpl_node(&vpl_node).unwrap();
		assert_eq!(args.data_source_path, "data.csv");
		assert_eq!(args.id_field_tiles, "id");
		assert_eq!(args.id_field_data, "id");
		assert!(args.replace_properties);
		assert!(args.include_id);
	}

	async fn run_test(input: &str) -> Result<String> {
		let temp_file = NamedTempFile::new("test.csv")?;
		let mut file = File::create(&temp_file)?;
		writeln!(&mut file, "data_id,value\n0,test")?;

		let parts = input.split(',').collect::<Vec<_>>();
		let replace = |value: &str, key: &str| {
			if value.is_empty() {
				String::from("")
			} else {
				format!("{key}={value}")
			}
		};

		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(
				&[
					"from_container filename=dummy.pbf |",
					"vectortiles_update_properties",
					&format!(
						"data_source_path=\"{}\"",
						temp_file.to_str().unwrap().replace("\\", "\\\\")
					),
					&replace(parts[0], "id_field_tiles"),
					&replace(parts[1], "id_field_data"),
					"layer_name=mock",
					&replace(parts[2], "replace_properties"),
					&replace(parts[3], "include_id"),
				]
				.join(" "),
			)
			.await?;

		let blob = operation.get_tile_data(&TileCoord3::new(0, 0, 0)?).await?.unwrap();
		let tile = VectorTile::from_blob(&blob)?;

		assert_eq!(tile.layers.len(), 1);
		assert_eq!(tile.layers[0].features.len(), 1);
		let properties = tile.layers[0].features[0].decode_properties(&tile.layers[0])?;
		let mut vec = operation.get_tilejson().as_pretty_lines(100);
		vec.insert(0, format!("{properties:?}"));
		Ok(vec.join("\n"))
	}

	#[tokio::test]
	async fn test_run_variation1() {
		assert_eq!(
			run_test("x,data_id,false,false").await.unwrap().split("\n").collect::<Vec<_>>(),
			["{\"filename\": String(\"dummy.pbf\"), \"value\": String(\"test\"), \"x\": UInt(0), \"y\": UInt(0), \"z\": UInt(0)}", 
			"{",
			"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
			"  \"maxzoom\": 8,",
			"  \"minzoom\": 0,",
			"  \"name\": \"mock vector source\",",
			"  \"tile_content\": \"vector\",",
			"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
			"  \"tile_schema\": \"other\",",
			"  \"tilejson\": \"3.0.0\"",
			"}"]
		);
	}

	#[tokio::test]
	async fn test_run_variation2() {
		assert_eq!(
			run_test("x,data_id,false,true").await.unwrap().split("\n").collect::<Vec<_>>(),
			["{\"data_id\": UInt(0), \"filename\": String(\"dummy.pbf\"), \"value\": String(\"test\"), \"x\": UInt(0), \"y\": UInt(0), \"z\": UInt(0)}", 
			"{",
			"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
			"  \"maxzoom\": 8,",
			"  \"minzoom\": 0,",
			"  \"name\": \"mock vector source\",",
			"  \"tile_content\": \"vector\",",
			"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
			"  \"tile_schema\": \"other\",",
			"  \"tilejson\": \"3.0.0\"",
			"}"]
		);
	}

	#[tokio::test]
	async fn test_run_variation3() {
		assert_eq!(
			run_test("x,data_id,true,false")
				.await
				.unwrap()
				.split("\n")
				.collect::<Vec<_>>(),
			[
				"{\"value\": String(\"test\")}",
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}

	#[tokio::test]
	async fn test_run_variation4() {
		assert_eq!(
			run_test("x,data_id,true,true")
				.await
				.unwrap()
				.split("\n")
				.collect::<Vec<_>>(),
			[
				"{\"data_id\": UInt(0), \"value\": String(\"test\")}",
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);
	}
}
