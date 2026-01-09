use crate::{
	PipelineFactory,
	operations::vector::traits::{RunnerTrait, build_transform},
	traits::{OperationFactoryTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use versatiles_container::TileSource;
use versatiles_core::TileJSON;
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filters vector tile layers based on a comma-separated list of layer names.
struct Args {
	/// Comma‑separated list of layer names that should be removed from the tiles, e.g.: filter="pois,ocean".
	filter: String,

	/// If set, inverts the filter logic (i.e., keeps only layers matching the filter).
	invert: Option<bool>,
}

#[derive(Debug)]
struct Runner {
	layer_set: HashSet<String>,
	invert: bool,
}

impl Runner {
	pub fn from_args(args: Args) -> Self {
		let layer_set: HashSet<String> = args.filter.split(',').map(|s| s.trim().to_string()).collect();

		Self {
			layer_set,
			invert: args.invert.unwrap_or(false),
		}
	}
}

impl RunnerTrait for Runner {
	#[context("Failed to run vector filter layers")]
	fn run(&self, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		tile
			.layers
			.retain(|layer| self.layer_set.contains(&layer.name) == self.invert);

		Ok(Some(tile))
	}
	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		tilejson
			.vector_layers
			.0
			.retain(|name, _| self.layer_set.contains(name) == self.invert);
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"vector_filter_layers"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		_factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSource>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		build_transform::<Runner>(source, Runner::from_args(args)).await
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;
	use versatiles_geometry::{geo::*, vector_tile::VectorTileLayer};

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

		fn extract_suffix(layer: &VectorTileLayer) -> Result<String> {
			let suffix = layer
				.name
				.strip_prefix("test_layer")
				.map(std::string::ToString::to_string)
				.ok_or_else(|| anyhow::anyhow!("Layer name does not start with 'test_layer': {}", layer.name))?;
			Ok(suffix)
		}

		let runner = Runner::from_args(Args {
			filter: "test_layer1".to_string(),
			invert: None,
		});

		let tile0 = VectorTile::new(vec![create_layer("1"), create_layer("2")]);
		let tile1 = runner.run(tile0).unwrap().unwrap();

		assert_eq!(tile1.layers.len(), 1);
		assert_eq!(extract_suffix(&tile1.layers[0]).unwrap(), "2");
	}

	#[test]
	fn test_args_from_vpl_node() {
		let vpl_node = VPLNode::try_from_str(r#"vector_filter_layers filter="temp,tomp" invert=true"#).unwrap();

		let args = Args::from_vpl_node(&vpl_node).unwrap();
		assert_eq!(args.filter, "temp,tomp");
		assert_eq!(args.invert, Some(true));
	}

	async fn run_test(filter: &str, invert: &str) -> Result<(String, String)> {
		let replace = |key: &str, value: &str| {
			if value.is_empty() {
				String::new()
			} else {
				format!("{key}={value}")
			}
		};

		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(
				&[
					"from_debug |",
					"vector_filter_layers",
					&replace("filter", filter),
					&replace("invert", invert),
				]
				.join(" "),
			)
			.await?;

		let mut stream = operation.get_tile_stream(TileBBox::new_full(0)?).await?;
		let tile = stream.next().await.unwrap().1.into_vector()?;
		let layer_names = tile
			.layers
			.iter()
			.map(|layer| layer.name.clone())
			.collect::<Vec<_>>()
			.join(",");

		let tilejson = operation.tilejson();
		let layer_ids = tilejson.vector_layers.layer_ids().join(",");

		Ok((layer_names, layer_ids))
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
				"Failed to get required property string 'filter' from VPL node 'vector_filter_layers'",
				"In operation 'vector_filter_layers' the parameter 'filter' is required.",
			]
		);
	}

	#[tokio::test]
	async fn test_filter_layer() {
		let (layers, json) = run_test("debug_z", "").await.unwrap();
		assert_eq!(layers, "background,debug_x,debug_y");
		assert_eq!(json, "background,debug_x,debug_y");
	}

	#[tokio::test]
	async fn test_filter_unknown_layer() {
		let (layers, json) = run_test("unknown", "").await.unwrap();
		assert_eq!(layers, "background,debug_z,debug_x,debug_y");
		assert_eq!(json, "background,debug_x,debug_y,debug_z");
	}

	#[tokio::test]
	async fn test_filter_and_invert() {
		let (layers, json) = run_test("debug_y", "true").await.unwrap();
		assert_eq!(layers, "debug_y");
		assert_eq!(json, "debug_y");
	}
}
