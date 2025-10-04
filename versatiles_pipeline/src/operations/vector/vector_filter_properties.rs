use crate::{
	PipelineFactory,
	operations::vector::traits::{RunnerTrait, build_transform},
	traits::{OperationFactoryTrait, OperationTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use regex::Regex;
use versatiles_core::tilejson::TileJSON;
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filters properties based on a regular expressions.
struct Args {
	/// A regular expression pattern that should match property names to be removed from all features.
	/// The property names contain the layer name as a prefix, e.g., `layer_name/property_name`,
	/// so an expression like `^layer_name/` will match all properties of that layer or
	/// `/name_.*$/` will match all properties starting with `name_` in all layers.
	regex: String,

	/// If set, inverts the filter logic (i.e., keeps only properties matching the filter).
	invert: Option<bool>,
}

#[derive(Debug)]
struct Runner {
	regex: Regex,
	invert: bool,
}

impl Runner {
	pub fn from_args(args: Args) -> Result<Self> {
		let regex = Regex::new(&args.regex).context("Failed to compile regex")?;

		Ok(Self {
			regex,
			invert: args.invert.unwrap_or(false),
		})
	}
}

impl RunnerTrait for Runner {
	fn run(&self, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		tile.layers.iter_mut().for_each(|layer| {
			let name = layer.name.clone();
			layer
				.filter_map_properties(|mut properties| {
					properties.retain(|key, _| self.regex.is_match(&format!("{name}/{key}")) == self.invert);
					Some(properties)
				})
				.unwrap();
		});

		Ok(Some(tile))
	}
	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		tilejson.vector_layers.iter_mut().for_each(|(name, layer)| {
			layer
				.fields
				.retain(|key, _| self.regex.is_match(&format!("{name}/{key}")) == self.invert);
		});
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"vector_filter_properties"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		_factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		build_transform::<Runner>(source, Runner::from_args(args)?).await
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;
	use versatiles_geometry::{GeoFeature, GeoProperties, GeoValue, Geometry, vector_tile::VectorTileLayer};

	fn extract_tile_properties(tile: &VectorTile) -> Vec<String> {
		let mut properties: Vec<String> = tile
			.layers
			.iter()
			.flat_map(|layer| {
				let name = layer.name.clone();
				layer.features.iter().flat_map(move |feature| {
					let p = feature.decode_properties(layer).unwrap();
					p.iter().map(|(k, _v)| format!("{name}/{k}")).collect::<Vec<_>>()
				})
			})
			.collect();
		properties.sort();
		properties.dedup();
		properties
	}

	fn extract_json_properties(tilejson: &TileJSON) -> Vec<String> {
		let mut properties: Vec<String> = tilejson
			.vector_layers
			.iter()
			.flat_map(|(name, layer)| {
				layer
					.fields
					.keys()
					.map(|key| format!("{name}/{key}"))
					.collect::<Vec<String>>()
			})
			.collect();
		properties.sort();
		properties.dedup();
		properties
	}

	#[tokio::test]
	async fn test_runner_run() {
		fn create_layer(suffix: &str) -> VectorTileLayer {
			let mut feature = GeoFeature::new(Geometry::new_example());
			feature.properties = GeoProperties::from(vec![
				("id", GeoValue::from(format!("feature_{suffix}"))),
				("property", GeoValue::from(format!("value_{suffix}"))),
			]);
			VectorTileLayer::from_features(format!("test_layer{suffix}"), vec![feature], 4096, 1).unwrap()
		}

		let runner = Runner::from_args(Args {
			regex: "index$".to_string(),
			invert: None,
		})
		.unwrap();

		let tile0 = VectorTile::new(vec![create_layer("1"), create_layer("2")]);
		let tile1 = runner.run(tile0).unwrap().unwrap();

		assert_eq!(
			extract_tile_properties(&tile1),
			[
				"test_layer1/id",
				"test_layer1/property",
				"test_layer2/id",
				"test_layer2/property",
			]
		);
	}

	async fn run_test(regex: &str, invert: &str) -> Result<(String, String)> {
		let replace = |key: &str, value: &str| {
			if value.is_empty() {
				String::from("")
			} else {
				format!("{key}=\"{value}\"")
			}
		};

		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(
				&[
					"from_debug |",
					"vector_filter_properties",
					&replace("regex", regex),
					&replace("invert", invert),
				]
				.join(" "),
			)
			.await?;

		let mut stream = operation.get_stream(TileBBox::new_full(0)?).await?;
		let tile = stream.next().await.unwrap().1.into_vector()?;

		Ok((
			extract_tile_properties(&tile).join(";"),
			extract_json_properties(operation.tilejson()).join(";"),
		))
	}

	#[tokio::test]
	async fn test_no_args() {
		let result = run_test("", "").await;
		assert_eq!(
			result.unwrap_err().to_string(),
			"Failed to get required property string 'regex' from VPL node 'vector_filter_properties'"
		);
	}

	fn split(s: String) -> Vec<String> {
		s.split(';').map(|s| s.to_string()).collect()
	}

	#[tokio::test]
	async fn test_filter_layer() {
		let (layers, json) = run_test("debug_z", "").await.unwrap();
		let result = [
			"debug_x/char",
			"debug_x/index",
			"debug_x/x",
			"debug_y/char",
			"debug_y/index",
			"debug_y/x",
		];
		assert_eq!(split(layers), result);
		assert_eq!(split(json), result);
	}

	#[tokio::test]
	async fn test_filter_property() {
		let (layers, json) = run_test("/index$", "").await.unwrap();
		let result = [
			"debug_x/char",
			"debug_x/x",
			"debug_y/char",
			"debug_y/x",
			"debug_z/char",
			"debug_z/x",
		];
		assert_eq!(split(layers), result);
		assert_eq!(split(json), result);
	}

	#[tokio::test]
	async fn test_filter_and_invert() {
		let (layers, json) = run_test("/index$", "true").await.unwrap();
		let result = ["debug_x/index", "debug_y/index", "debug_z/index"];
		assert_eq!(split(layers), result);
		assert_eq!(split(json), result);
	}
}
