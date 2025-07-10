use crate::{
	PipelineFactory,
	operations::vector::traits::{RunnerTrait, TransformOp},
	traits::{OperationFactoryTrait, OperationTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{collections::HashSet, sync::Arc};
use versatiles_core::{tilejson::TileJSON, types::TileType};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Updates properties of vector tile features using data from an external source (e.g., CSV file). Matches features based on an ID field.
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
	pub fn from_args(args: &Args) -> Self {
		let layer_set: HashSet<String> = args.filter.split(',').map(|s| s.trim().to_string()).collect();

		Self {
			layer_set,
			invert: args.invert.unwrap_or(false),
		}
	}
}

impl RunnerTrait for Runner {
	fn run(&self, mut tile: VectorTile) -> Result<VectorTile> {
		tile
			.layers
			.retain(|layer| self.layer_set.contains(&layer.name) == self.invert);

		Ok(tile)
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
		"vectortiles_filter_layers"
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

		let parameters = source.get_parameters().clone();
		ensure!(
			parameters.tile_format.get_type() == TileType::Vector,
			"source must be vector tiles"
		);

		let runner = Arc::new(Runner::from_args(&args));

		let mut tilejson = source.get_tilejson().clone();
		runner.update_tilejson(&mut tilejson);
		tilejson.update_from_reader_parameters(&parameters);

		Ok(Box::new(TransformOp::<Runner> {
			runner,
			source,
			params: parameters,
			tilejson,
		}) as Box<dyn OperationTrait>)
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use versatiles_core::types::TileCoord3;
	use versatiles_geometry::{GeoFeature, GeoProperties, GeoValue, Geometry, vector_tile::VectorTileLayer};

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
				.map(|s| s.to_string())
				.ok_or_else(|| anyhow::anyhow!("Layer name does not start with 'test_layer': {}", layer.name))?;
			Ok(suffix)
		}

		let runner = Runner::from_args(&Args {
			filter: "test_layer1".to_string(),
			invert: None,
		});

		let tile0 = VectorTile::new(vec![create_layer("1"), create_layer("2")]);
		let tile1 = runner.run(tile0).unwrap();

		assert_eq!(tile1.layers.len(), 1);
		assert_eq!(extract_suffix(&tile1.layers[0]).unwrap(), "2");
	}

	#[test]
	fn test_args_from_vpl_node() {
		let vpl_node = VPLNode::from_str(r##"vectortiles_filter_layers filter="temp,tomp" invert=true"##).unwrap();

		let args = Args::from_vpl_node(&vpl_node).unwrap();
		assert_eq!(args.filter, "temp,tomp");
		assert_eq!(args.invert, Some(true));
	}

	async fn run_test(filter: &str, invert: &str) -> Result<(String, Vec<String>)> {
		let replace = |key: &str, value: &str| {
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
					"from_debug |",
					"vectortiles_filter_layers",
					&replace("filter", filter),
					&replace("invert", invert),
				]
				.join(" "),
			)
			.await?;

		let blob = operation.get_tile_data(&TileCoord3::new(0, 0, 0)?).await?.unwrap();
		let tile = VectorTile::from_blob(&blob)?;
		let layer_names = tile.layers.iter().map(|layer| layer.name.clone()).collect::<Vec<_>>();

		let tilejson = operation.get_tilejson().as_pretty_lines(100);

		Ok((layer_names.join(","), tilejson))
	}

	#[tokio::test]
	async fn test_no_args() {
		let result = run_test("", "").await;
		assert_eq!(
			result.unwrap_err().to_string(),
			"In operation 'vectortiles_filter_layers' the parameter 'filter' is required."
		);
	}

	#[tokio::test]
	async fn test_filter_layer() {
		let result = run_test("debug_z", "").await.unwrap();
		assert_eq!(result.0, "background,debug_x,debug_y");
		assert_eq!(
			result.1,
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 30,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\",",
				"  \"vector_layers\": [",
				"    { \"fields\": {  }, \"id\": \"background\", \"maxzoom\": 30, \"minzoom\": 0 },",
				"    {",
				"      \"fields\": { \"char\": \"which character\", \"index\": \"index of char\", \"position\": \"x value\" },",
				"      \"id\": \"debug_x\",",
				"      \"maxzoom\": 30,",
				"      \"minzoom\": 0",
				"    },",
				"    {",
				"      \"fields\": { \"char\": \"which character\", \"index\": \"index of char\", \"position\": \"x value\" },",
				"      \"id\": \"debug_y\",",
				"      \"maxzoom\": 30,",
				"      \"minzoom\": 0",
				"    }",
				"  ]",
				"}",
			]
		);
	}

	#[tokio::test]
	async fn test_filter_unknown_layer() {
		let result = run_test("unknown", "").await.unwrap();
		assert_eq!(result.0, "background,debug_z,debug_x,debug_y");
		assert_eq!(
			result.1,
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 30,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\",",
				"  \"vector_layers\": [",
				"    { \"fields\": {  }, \"id\": \"background\", \"maxzoom\": 30, \"minzoom\": 0 },",
				"    {",
				"      \"fields\": { \"char\": \"which character\", \"index\": \"index of char\", \"position\": \"x value\" },",
				"      \"id\": \"debug_x\",",
				"      \"maxzoom\": 30,",
				"      \"minzoom\": 0",
				"    },",
				"    {",
				"      \"fields\": { \"char\": \"which character\", \"index\": \"index of char\", \"position\": \"x value\" },",
				"      \"id\": \"debug_y\",",
				"      \"maxzoom\": 30,",
				"      \"minzoom\": 0",
				"    },",
				"    {",
				"      \"fields\": { \"char\": \"which character\", \"index\": \"index of char\", \"position\": \"x value\" },",
				"      \"id\": \"debug_z\",",
				"      \"maxzoom\": 30,",
				"      \"minzoom\": 0",
				"    }",
				"  ]",
				"}",
			]
		);
	}

	#[tokio::test]
	async fn test_filter_and_invert() {
		let result = run_test("debug_y", "true").await.unwrap();
		assert_eq!(result.0, "debug_y");
		assert_eq!(
			result.1,
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 30,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\",",
				"  \"vector_layers\": [",
				"    {",
				"      \"fields\": { \"char\": \"which character\", \"index\": \"index of char\", \"position\": \"x value\" },",
				"      \"id\": \"debug_y\",",
				"      \"maxzoom\": 30,",
				"      \"minzoom\": 0",
				"    }",
				"  ]",
				"}",
			]
		);
	}
}
