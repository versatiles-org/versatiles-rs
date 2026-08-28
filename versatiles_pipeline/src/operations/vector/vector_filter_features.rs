use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

use anyhow::{Result, anyhow};
use cel_interpreter::{
	Context as CelContext, Program, Value as CelValue,
	objects::{Key as CelKey, Map as CelMap},
};
use versatiles_container::TileSource;
use versatiles_core::TileJSON;
use versatiles_derive::context;
use versatiles_geometry::{
	geo::{GeoProperties, GeoValue},
	vector_tile::VectorTile,
};

use crate::{
	PipelineFactory,
	operations::transform::{AsTileTransform, TransformOp, VectorTransform},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Drops vector features in selected layers that do not satisfy a boolean expression.
///
/// Features in layers outside `layer` pass through untouched.
///
/// ### Examples
///
/// ```vpl
/// vector_filter_features layer=["place"] expr="name == 'Berlin'"
/// vector_filter_features layer=["poi"]   expr="population >= 1000"
/// vector_filter_features layer=["road"]  expr="highway in ['primary','secondary']"
/// vector_filter_features layer=["place"] expr="name.matches('^St\\.')"
/// vector_filter_features layer=["poi"]   expr="name != null && name != ''"
/// vector_filter_features layer=["addr"]  expr="props['addr:street'] == 'Hauptstr.'"
/// ```
///
/// ### Expression language
///
/// `expr` is a boolean [CEL (Common Expression
/// Language)](https://github.com/google/cel-spec) expression, evaluated once per
/// feature.
///
/// **Types** — bool (`true`, `false`), int / uint (`42`, `-7`, `1000u`), double
/// (`3.14`, `-0.5`, `1e-6`), string (`'hello'` or `"hello"`), list (`[1, 2, 3]`,
/// `['a', 'b']`), map (`m['key']` or `m.key`), and `null`.
///
/// **Operators** — equality `==` `!=`, ordering `<` `<=` `>` `>=`, logical `&&`
/// `||` `!`, membership `x in [1, 2, 3]`, and `s.matches('pattern')` for a regex
/// in RE2 syntax, matched anywhere in `s`.
///
/// ### Accessing feature properties
///
/// Properties whose names are valid CEL identifiers — letters, digits and
/// underscore — are exposed as top-level variables:
///
/// ```vpl
/// vector_filter_features layer=["place"] expr="name == 'Berlin'"
/// ```
///
/// For keys containing `:`, `-`, `.`, or anything else that is not an
/// identifier, use the `props` map:
///
/// ```vpl
/// vector_filter_features layer=["addr"] expr="props['addr:street'] == 'Hauptstr.'"
/// ```
///
/// ### Missing keys
///
/// A property a feature does not carry resolves to `null` for identifier-safe
/// access, so compare against `null` to say explicitly whether such features are
/// kept or dropped:
///
/// ```vpl
/// vector_filter_features layer=["place"] expr="name != null && name != ''"
/// ```
///
/// The `has()` macro asks the same question of an identifier-safe key, and `in`
/// of any key:
///
/// ```vpl
/// vector_filter_features layer=["place"] expr="has(props.name)"
/// vector_filter_features layer=["addr"]  expr="'addr:street' in props"
/// ```
///
/// The [CEL language
/// spec](https://github.com/google/cel-spec/blob/master/doc/langdef.md) has the
/// full grammar, built-in functions and string methods.
struct Args {
	/// Layers the expression applies to, for example `layer=["poi","place"]`.
	layer: Vec<String>,

	/// Boolean CEL expression over the feature's properties.
	expr: CelExpression,
}

/// A CEL expression, compiled where the value is decoded.
///
/// An expression that does not compile used to be found when the operation was
/// built; `check` finds it now, without building anything (#260). The text is
/// kept rather than the [`Program`], which is not `Clone`; both this and the
/// runner call [`compile_cel`], so what `check` accepts and what builds are the
/// same set.
#[derive(Clone, Debug)]
pub struct CelExpression(String);

impl CelExpression {
	/// The expression as it was written.
	#[must_use]
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl TryFrom<&str> for CelExpression {
	type Error = anyhow::Error;

	fn try_from(expr: &str) -> Result<Self> {
		compile_cel(expr)?;
		Ok(Self(expr.to_string()))
	}
}

/// Compiles a CEL expression, containing the parser's panics.
///
/// `cel-parser 0.10.1` panics on some malformed inputs instead of returning
/// `ParseErrors`. Containing the panic keeps a bad expression from taking down
/// the whole pipeline — and now also from taking down an editor asking `check`
/// what is wrong with the line someone is halfway through typing.
///
/// The recovery is silent to the caller but not to the terminal: `catch_unwind`
/// still runs the panic hook, so a panicking expression prints
/// `thread … panicked at antlr4rust …` to stderr. Silencing it would mean
/// installing a process-global panic hook from inside a library, which is the
/// shape of problem #261 was filed about. Whoever owns the process can set a
/// hook; this function will not.
fn compile_cel(expr: &str) -> Result<Program> {
	std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Program::compile(expr)))
		.map_err(|panic_payload| {
			// `catch_unwind` returns `Box<dyn Any + Send>`; extract a message if the panic
			// carried one (Rust panics with string literals or `format!`-ed Strings).
			let detail = panic_payload
				.downcast_ref::<String>()
				.map(String::as_str)
				.or_else(|| panic_payload.downcast_ref::<&'static str>().copied())
				.unwrap_or("no panic message");
			anyhow!(
				"Failed to compile CEL expression:\n  {expr}\n\n\
				 Parser crashed (likely malformed CEL input): {detail}\n\n\
				 Common causes: unmatched brackets, trailing operators, unsupported tokens. \
				 Run `versatiles help pipeline` for the CEL operator cheat-sheet."
			)
		})?
		.map_err(|e| {
			// `ParseErrors::Display` already renders `ERROR: <input>:L:C: msg` plus a source
			// snippet with a caret. Put the expression and the report on separate lines so
			// the caret alignment survives and terminal output stays readable.
			anyhow!("Failed to compile CEL expression:\n  {expr}\n\n{e}")
		})
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
		let program = compile_cel(args.expr.as_str())?;

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

impl VectorTransform for Runner {
	const TAG: &'static str = "vector_filter_features";

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

impl Runner {
	#[context("Building vector_filter_features operation in VPL node {:?}", vpl_node.name)]
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

crate::operations::macros::define_transform_factory!("vector_filter_features", Args, Runner, requires: Vector);

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;
	use versatiles_geometry::{
		geo::{GeoFeature, example_geometry},
		vector_tile::VectorTileLayer,
	};

	use super::*;

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
			expr: CelExpression::try_from(expr)?,
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
		assert_eq!(a.expr.as_str(), "true");
	}

	#[test]
	fn test_expr_compile_error_on_panic_path_is_helpful() {
		// Incomplete trailing operator — `cel-parser 0.10.1` panics on this input.
		// The error should: (a) echo the user's expression, (b) flag likely-malformed input,
		// (c) point them at `versatiles help pipeline` for reference material.
		//
		// Asked of the type rather than of `Runner`: an expression that does not
		// compile can no longer reach one, which is the point of the type.
		let err = CelExpression::try_from("population >=").unwrap_err();
		assert_eq!(
			err.to_string().split('\n').collect::<Vec<_>>(),
			[
				"Failed to compile CEL expression:",
				"  population >=",
				"",
				"Parser crashed (likely malformed CEL input): internal error: entered unreachable code: should have been properly implemented by generated context when reachable",
				"",
				"Common causes: unmatched brackets, trailing operators, unsupported tokens. Run `versatiles help pipeline` for the CEL operator cheat-sheet.",
			]
		);
	}

	#[test]
	fn test_expr_compile_error_on_err_path_includes_location() {
		// Unknown / unsupported token — cel-parser returns ParseErrors with line/column info
		// rather than panicking. The error should surface that location report to the user.
		let err = CelExpression::try_from("foo @@").unwrap_err();
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

	#[tokio::test]
	async fn output_tiles_pass_mvt_validation() -> Result<()> {
		use crate::helpers::assert_tiles_valid;
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features layer=["debug_x"] expr="true""#)
			.await?;
		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}
}
