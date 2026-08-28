use anyhow::{Context, Result};
use regex::Regex;
use versatiles_container::TileSource;
use versatiles_core::TileJSON;
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

use crate::{
	PipelineFactory,
	operations::transform::{AsTileTransform, TransformOp, VectorTransform},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Removes feature properties from vector tiles by matching their names against a regex.
///
/// A property's name is matched with its layer as a prefix, in the form
/// `layer_name/property_name`. That makes it possible to target one layer —
/// `regex="^places/"` drops every property of the `places` layer — or to reach
/// across all of them, as `regex="/name_.*$"` does for every property starting
/// with `name_`.
struct Args {
	/// Regular expression matched against each property's prefixed name.
	regex: RegexPattern,

	/// Whether to keep the matching properties instead of removing them. Defaults to `false`.
	#[vpl(default = "false")]
	invert: Option<bool>,
}

/// A regular expression, compiled where the value is decoded.
///
/// A pattern that does not compile used to be found when the operation was
/// built; now `check` finds it, without building anything (#260). The compiled
/// form is kept rather than the text, so the pattern is compiled once.
#[derive(Clone, Debug)]
pub struct RegexPattern(Regex);

impl TryFrom<&str> for RegexPattern {
	type Error = anyhow::Error;

	fn try_from(pattern: &str) -> Result<Self> {
		Regex::new(pattern).map(Self).context("Failed to compile regex")
	}
}

#[derive(Debug)]
struct Runner {
	regex: Regex,
	invert: bool,
}

impl Runner {
	pub fn from_args(args: &Args) -> Result<Self> {
		Ok(Self {
			regex: args.regex.0.clone(),
			invert: args.invert.unwrap_or(false),
		})
	}
}

impl VectorTransform for Runner {
	const TAG: &'static str = "vector_filter_properties";

	#[context("Failed to run vector filter properties")]
	fn run(&self, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		tile.layers.iter_mut().try_for_each(|layer| {
			let name = layer.name.clone();
			layer.filter_map_properties(|mut properties| {
				properties.retain(|key, _| self.regex.is_match(&format!("{name}/{key}")) == self.invert);
				Some(properties)
			})
		})?;

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

impl Runner {
	#[context("Building vector_filter_properties operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<AsTileTransform<Runner>>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		Ok(TransformOp::new(
			source,
			AsTileTransform(Runner::from_args(&args)?),
			factory.runtime(),
		))
	}
}

crate::operations::macros::define_transform_factory!("vector_filter_properties", Args, Runner, requires: Vector);

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;
	use versatiles_geometry::{geo::*, vector_tile::VectorTileLayer};

	use super::*;

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
			let mut feature = GeoFeature::new(example_geometry());
			feature.properties = GeoProperties::from(vec![
				("id", GeoValue::from(format!("feature_{suffix}"))),
				("property", GeoValue::from(format!("value_{suffix}"))),
			]);
			VectorTileLayer::from_features(format!("test_layer{suffix}"), vec![feature], 4096, 1).unwrap()
		}

		let runner = Runner::from_args(&Args {
			regex: RegexPattern::try_from("index$").unwrap(),
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
				String::new()
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

		let mut stream = operation.tile_stream(TileBBox::new_full(0)?).await?;
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
			result
				.unwrap_err()
				.chain()
				.map(std::string::ToString::to_string)
				.collect::<Vec<_>>(),
			[
				"Failed to create reader from VPL",
				"Failed to build pipeline from VPL",
				"Failed to create transform operation from VPL node",
				"Building vector_filter_properties operation in VPL node \"vector_filter_properties\"",
				"Failed to get required property enum 'regex' from VPL node 'vector_filter_properties'",
				"In operation 'vector_filter_properties' the parameter 'regex' is required but missing.",
			]
		);
	}

	fn split(s: &str) -> Vec<String> {
		s.split(';').map(std::string::ToString::to_string).collect()
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
		assert_eq!(split(&layers), result);
		assert_eq!(split(&json), result);
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
		assert_eq!(split(&layers), result);
		assert_eq!(split(&json), result);
	}

	#[tokio::test]
	async fn test_filter_and_invert() {
		let (layers, json) = run_test("/index$", "true").await.unwrap();
		let result = ["debug_x/index", "debug_y/index", "debug_z/index"];
		assert_eq!(split(&layers), result);
		assert_eq!(split(&json), result);
	}

	#[tokio::test]
	async fn output_tiles_pass_mvt_validation() -> Result<()> {
		use crate::helpers::assert_tiles_valid;
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_properties regex="/^char/""#)
			.await?;
		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}
}
