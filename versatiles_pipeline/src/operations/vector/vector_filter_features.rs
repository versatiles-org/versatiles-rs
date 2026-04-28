use crate::{
	PipelineFactory,
	factory::{OperationFactoryTrait, TransformOperationFactoryTrait},
	operations::vector::traits::{RunnerTrait, build_transform},
	vpl::VPLNode,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use cel_interpreter::{
	Context as CelContext, Program, Value as CelValue,
	objects::{Key as CelKey, Map as CelMap},
};
use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};
use versatiles_container::TileSource;
use versatiles_core::TileJSON;
use versatiles_derive::context;
use versatiles_geometry::{
	geo::{GeoProperties, GeoValue},
	vector_tile::VectorTile,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Drops vector features in selected layers that do not satisfy a boolean CEL expression.
/// Features in layers outside `layer` pass through untouched.
///
/// ### Examples
///
/// ```text
/// vector_filter_features layer=["place"] expr="name == 'Berlin'"
/// vector_filter_features layer=["poi"]   expr="population >= 1000"
/// vector_filter_features layer=["road"]  expr="highway in ['primary','secondary']"
/// vector_filter_features layer=["place"] expr="name.matches('^St\\.')"
/// vector_filter_features layer=["poi"]   expr="name != null && name != ''"
/// vector_filter_features layer=["addr"]  expr="props['addr:street'] == 'Hauptstr.'"
/// ```
struct Args {
	/// Layers the expression applies to, as a VPL array of strings.
	/// Features in all other layers are left unchanged.
	/// Example: `layer=["poi","place"]`.
	layer: Vec<String>,

	/// CEL (Common Expression Language) boolean expression.
	/// Feature properties are available as `props["key"]`; properties whose names are
	/// valid CEL identifiers (letters, digits, underscore) are also exposed as top-level
	/// identifiers. Missing keys resolve to null; use `name != null` (for identifier-safe
	/// keys) or `has(props.key)` (for any key) for explicit presence checks.
	/// See `versatiles help` for a CEL operator cheat-sheet.
	expr: String,
}

#[derive(Debug)]
struct Runner {
	layer_set: HashSet<String>,
	program: Program,
	/// Top-level identifiers referenced by the expression, other than `props`.
	/// Bound to the feature's property value (or `Null` if absent) on each evaluation.
	referenced_vars: Vec<String>,
	/// Whether the expression references the reserved `props` map. The map is only built
	/// per-feature when needed.
	binds_props: bool,
}

impl Runner {
	pub fn from_args(args: &Args) -> Result<Self> {
		// `cel-parser 0.10.1` panics on some malformed inputs instead of returning ParseErrors.
		// Containing the panic here keeps a bad expression from taking down the whole pipeline
		// and preserves our contract: CEL errors surface cleanly at build time.
		let program = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Program::compile(&args.expr)))
			.map_err(|panic_payload| {
				// `catch_unwind` returns `Box<dyn Any + Send>`; extract a message if the panic
				// carried one (Rust panics with string literals or `format!`-ed Strings).
				let detail = panic_payload
					.downcast_ref::<String>()
					.map(String::as_str)
					.or_else(|| panic_payload.downcast_ref::<&'static str>().copied())
					.unwrap_or("no panic message");
				anyhow!(
					"Failed to compile CEL expression:\n  {}\n\n\
					 Parser crashed (likely malformed CEL input): {detail}\n\n\
					 Common causes: unmatched brackets, trailing operators, unsupported tokens. \
					 Run `versatiles help` for the CEL operator cheat-sheet.",
					args.expr
				)
			})?
			.map_err(|e| {
				// `ParseErrors::Display` already renders `ERROR: <input>:L:C: msg` plus a source
				// snippet with a caret. Put the expression and the report on separate lines so
				// the caret alignment survives and terminal output stays readable.
				anyhow!("Failed to compile CEL expression:\n  {}\n\n{e}", args.expr)
			})?;

		let refs = program.references();
		let binds_props = refs.has_variable("props");
		let referenced_vars: Vec<String> = refs
			.variables()
			.into_iter()
			.filter(|v| *v != "props")
			.map(String::from)
			.collect();

		Ok(Self {
			layer_set: args.layer.iter().cloned().collect(),
			program,
			referenced_vars,
			binds_props,
		})
	}

	fn evaluate(&self, props: &GeoProperties) -> bool {
		let mut ctx = CelContext::default();

		if self.binds_props {
			let mut map: HashMap<CelKey, CelValue> = HashMap::with_capacity(props.len());
			for (key, value) in props.iter() {
				map.insert(CelKey::from(key.clone()), geo_to_cel(value));
			}
			ctx.add_variable_from_value("props", CelValue::Map(CelMap { map: Arc::new(map) }));
		}

		for name in &self.referenced_vars {
			let value = props.get(name.as_str()).map_or(CelValue::Null, geo_to_cel);
			ctx.add_variable_from_value(name.clone(), value);
		}

		matches!(self.program.execute(&ctx), Ok(CelValue::Bool(true)))
	}
}

fn geo_to_cel(v: &GeoValue) -> CelValue {
	match v {
		GeoValue::Bool(b) => CelValue::Bool(*b),
		GeoValue::Int(i) => CelValue::Int(*i),
		GeoValue::UInt(u) => CelValue::UInt(*u),
		GeoValue::Float(f) => CelValue::Float(f64::from(*f)),
		GeoValue::Double(d) => CelValue::Float(*d),
		GeoValue::String(s) => CelValue::String(Arc::new(s.clone())),
		GeoValue::Null => CelValue::Null,
	}
}

impl RunnerTrait for Runner {
	#[context("Failed to run vector_filter_features")]
	fn run(&self, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		tile.layers.retain_mut(|layer| {
			if !self.layer_set.contains(&layer.name) {
				return true;
			}
			let _ = layer.filter_map_properties(|props| if self.evaluate(&props) { Some(props) } else { None });
			!layer.features.is_empty()
		});

		if tile.layers.is_empty() {
			Ok(None)
		} else {
			Ok(Some(tile))
		}
	}

	fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn docs(&self) -> String {
		Args::docs()
	}
	fn tag_name(&self) -> &str {
		"vector_filter_features"
	}
	#[cfg(feature = "codegen")]
	fn field_metadata(&self) -> Vec<crate::vpl::VPLFieldMeta> {
		Args::field_metadata()
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
		build_transform::<Runner>(source, Runner::from_args(&args)?).await
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use super::*;
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;
	use versatiles_geometry::{
		geo::{GeoFeature, example_geometry},
		vector_tile::VectorTileLayer,
	};

	fn feature(props: Vec<(&str, GeoValue)>) -> GeoFeature {
		let mut f = GeoFeature::new(example_geometry());
		f.properties = GeoProperties::from(props);
		f
	}

	fn layer(name: &str, features: Vec<GeoFeature>) -> VectorTileLayer {
		VectorTileLayer::from_features(name.to_string(), features, 4096, 1).unwrap()
	}

	fn run_expr(layers: &[&str], expr: &str, tile: VectorTile) -> Result<Option<VectorTile>> {
		let runner = Runner::from_args(&Args {
			layer: layers.iter().map(|s| (*s).to_string()).collect(),
			expr: expr.to_string(),
		})?;
		runner.run(tile)
	}

	/// Returns `(layer_name, feature_count)` pairs, sorted by layer name.
	fn summarise(tile: &VectorTile) -> Vec<(String, usize)> {
		let mut s: Vec<_> = tile.layers.iter().map(|l| (l.name.clone(), l.features.len())).collect();
		s.sort();
		s
	}

	#[test]
	fn test_args_requires_layer_and_expr() {
		let n = VPLNode::try_from_str(r#"vector_filter_features expr="true""#).unwrap();
		assert!(Args::from_vpl_node(&n).is_err());

		let n = VPLNode::try_from_str(r#"vector_filter_features layer=["poi"]"#).unwrap();
		assert!(Args::from_vpl_node(&n).is_err());
	}

	#[test]
	fn test_args_parses_layer_array() {
		let n = VPLNode::try_from_str(r#"vector_filter_features layer=["poi","place"] expr="true""#).unwrap();
		let a = Args::from_vpl_node(&n).unwrap();
		assert_eq!(a.layer, vec!["poi".to_string(), "place".to_string()]);
		assert_eq!(a.expr, "true");
	}

	#[test]
	fn test_expr_compile_error_on_panic_path_is_helpful() {
		// Incomplete trailing operator — `cel-parser 0.10.1` panics on this input.
		// The error should: (a) echo the user's expression, (b) flag likely-malformed input,
		// (c) point them at `versatiles help` for reference material.
		let err = Runner::from_args(&Args {
			layer: vec!["x".into()],
			expr: "population >=".into(),
		})
		.unwrap_err();
		assert_eq!(
			err.to_string().split('\n').collect::<Vec<_>>(),
			[
				"Failed to compile CEL expression:",
				"  population >=",
				"",
				"Parser crashed (likely malformed CEL input): internal error: entered unreachable code: should have been properly implemented by generated context when reachable",
				"",
				"Common causes: unmatched brackets, trailing operators, unsupported tokens. Run `versatiles help` for the CEL operator cheat-sheet.",
			]
		);
	}

	#[test]
	fn test_expr_compile_error_on_err_path_includes_location() {
		// Unknown / unsupported token — cel-parser returns ParseErrors with line/column info
		// rather than panicking. The error should surface that location report to the user.
		let err = Runner::from_args(&Args {
			layer: vec!["x".into()],
			expr: "foo @@".into(),
		})
		.unwrap_err();
		assert_eq!(
			err.to_string().split('\n').collect::<Vec<_>>(),
			[
				"Failed to compile CEL expression:",
				"  foo @@",
				"",
				"ERROR: <input>:1:5: Syntax error: token recognition error at: '@'",
				"| foo @@",
				"| ....^",
				"ERROR: <input>:1:6: Syntax error: token recognition error at: '@'",
				"| foo @@",
				"| .....^",
			]
		);
	}

	#[test]
	fn test_keeps_feature_when_predicate_true() {
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("population", GeoValue::Int(500))]),
				feature(vec![("population", GeoValue::Int(2000))]),
			],
		)]);
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap().unwrap();
		assert_eq!(summarise(&out), vec![("poi".to_string(), 1)]);
	}

	#[test]
	fn test_drops_feature_when_predicate_false() {
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("population", GeoValue::Int(500))]),
				feature(vec![("population", GeoValue::Int(600))]),
			],
		)]);
		// All drop: tile becomes empty → Ok(None).
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap();
		assert!(out.is_none());
	}

	#[test]
	fn test_out_of_scope_layer_untouched() {
		let tile = VectorTile::new(vec![
			layer("poi", vec![feature(vec![("population", GeoValue::Int(10))])]),
			layer("road", vec![feature(vec![("highway", GeoValue::from("service"))])]),
		]);
		// `expr` would drop the poi feature by `population < 1000`, but road is out of scope and untouched.
		let out = run_expr(&["road"], "highway == 'primary'", tile).unwrap().unwrap();
		// road feature does not match so road drops; poi (out of scope) passes through intact.
		assert_eq!(summarise(&out), vec![("poi".to_string(), 1)]);
	}

	#[test]
	fn test_drops_empty_layer_but_keeps_others() {
		let tile = VectorTile::new(vec![
			layer("poi", vec![feature(vec![("population", GeoValue::Int(10))])]),
			layer("road", vec![feature(vec![("highway", GeoValue::from("primary"))])]),
		]);
		// poi predicate drops its only feature; road is out of scope and survives.
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap().unwrap();
		assert_eq!(summarise(&out), vec![("road".to_string(), 1)]);
	}

	#[test]
	fn test_drops_empty_tile_when_all_layers_empty() {
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![feature(vec![("population", GeoValue::Int(10))])],
		)]);
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap();
		assert!(out.is_none(), "all layers filtered empty should drop the tile");
	}

	#[test]
	fn test_missing_property_is_dropped() {
		// Feature lacks `population`; `name == ...` expression has nothing to match against.
		let tile = VectorTile::new(vec![layer("poi", vec![feature(vec![("other", GeoValue::from("x"))])])]);
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap();
		assert!(out.is_none());
	}

	#[test]
	fn test_missing_property_with_null_check_keeps_feature() {
		// Users can keep features whose property is missing by comparing to null.
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("other", GeoValue::from("x"))]), // `name` missing → Null
				feature(vec![("name", GeoValue::from("Berlin"))]),
			],
		)]);
		let out = run_expr(&["poi"], "name == null || name == 'Berlin'", tile)
			.unwrap()
			.unwrap();
		assert_eq!(summarise(&out), vec![("poi".to_string(), 2)]);
	}

	#[test]
	fn test_key_in_props_map() {
		// For non-identifier keys (like OSM `addr:street`), `'key' in props` checks presence.
		let tile = VectorTile::new(vec![layer(
			"addr",
			vec![
				feature(vec![("addr:street", GeoValue::from("Hauptstr."))]),
				feature(vec![("other", GeoValue::from("x"))]),
			],
		)]);
		let out = run_expr(&["addr"], "'addr:street' in props", tile).unwrap().unwrap();
		assert_eq!(summarise(&out), vec![("addr".to_string(), 1)]);
	}

	#[test]
	fn test_has_on_props_map() {
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("name", GeoValue::from("Berlin"))]),
				feature(vec![("other", GeoValue::from("x"))]), // name missing
			],
		)]);
		let out = run_expr(&["poi"], "has(props.name)", tile).unwrap().unwrap();
		assert_eq!(summarise(&out), vec![("poi".to_string(), 1)]);
	}

	#[test]
	fn test_special_char_property_via_props_map() {
		let tile = VectorTile::new(vec![layer(
			"addr",
			vec![
				feature(vec![("addr:street", GeoValue::from("Hauptstr."))]),
				feature(vec![("addr:street", GeoValue::from("Nebenstr."))]),
			],
		)]);
		let out = run_expr(&["addr"], "props['addr:street'] == 'Hauptstr.'", tile)
			.unwrap()
			.unwrap();
		assert_eq!(summarise(&out), vec![("addr".to_string(), 1)]);
	}

	#[test]
	fn test_in_operator() {
		let tile = VectorTile::new(vec![layer(
			"road",
			vec![
				feature(vec![("highway", GeoValue::from("primary"))]),
				feature(vec![("highway", GeoValue::from("residential"))]),
				feature(vec![("highway", GeoValue::from("secondary"))]),
			],
		)]);
		let out = run_expr(&["road"], "highway in ['primary','secondary']", tile)
			.unwrap()
			.unwrap();
		assert_eq!(summarise(&out), vec![("road".to_string(), 2)]);
	}

	#[test]
	fn test_matches_operator() {
		let tile = VectorTile::new(vec![layer(
			"place",
			vec![
				feature(vec![("name", GeoValue::from("St. Mary"))]),
				feature(vec![("name", GeoValue::from("Berlin"))]),
				feature(vec![("name", GeoValue::from("St. Gallen"))]),
			],
		)]);
		let out = run_expr(&["place"], r"name.matches('^St\\.')", tile).unwrap().unwrap();
		assert_eq!(summarise(&out), vec![("place".to_string(), 2)]);
	}

	/// End-to-end via PipelineFactory: parses VPL, builds the op, streams tiles,
	/// and checks the transformed output.
	#[tokio::test]
	async fn test_integration_via_vpl() -> Result<()> {
		let factory = PipelineFactory::new_dummy();

		// `from_debug format=mvt` produces layers: background, debug_x, debug_y, debug_z.
		// Apply the filter only to debug_x; other layers should pass through.
		let op = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features layer=["debug_x"] expr="false""#)
			.await?;

		let mut stream = op.tile_stream(TileBBox::new_full(0)?).await?;
		let tile = stream.next().await.unwrap().1.into_vector()?;
		let summary: Vec<String> = tile.layers.iter().map(|l| l.name.clone()).collect();

		// debug_x should be gone; others remain.
		assert!(!summary.contains(&"debug_x".to_string()), "debug_x should be dropped");
		assert!(summary.contains(&"debug_y".to_string()));
		assert!(summary.contains(&"debug_z".to_string()));
		Ok(())
	}

	#[tokio::test]
	async fn test_integration_compile_error_surfaces() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features layer=["debug_x"] expr="population >=""#)
			.await;
		assert!(result.is_err(), "invalid CEL should fail at pipeline build time");
	}
}
