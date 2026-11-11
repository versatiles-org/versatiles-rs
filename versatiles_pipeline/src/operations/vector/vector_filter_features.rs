use std::collections::HashSet;

use anyhow::Result;
use versatiles_container::{Tile, TileSource};
use versatiles_core::{TileCoord, TileJSON};
use versatiles_derive::context;
use versatiles_geometry::{geo::GeoProperties, vector_tile::VectorTile};

use crate::{
	PipelineFactory,
	helpers::feature_expr::{CelExpression, ExprScope, FeatureExpr},
	operations::{removed::refuse_removed_parameters, transform::TileTransform},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Drops vector features that do not satisfy a boolean expression.
///
/// The expression is evaluated for every feature of every layer. Most filters
/// are written for one layer, so they go in a `vector_branch` that hands that
/// layer over and leaves the others as they are:
///
/// ```vpl
/// from_container filename="osm.versatiles"
///   | vector_branch expr="$layer == 'place'" [
///       vector_filter_features expr="name == 'Berlin'"
///     ]
/// ```
///
/// The same in one expression is `$layer != 'place' || name == 'Berlin'`, which
/// keeps every feature of the other layers. The branch costs less, because it
/// hands over whole layers without evaluating the expression for each of their
/// features.
///
/// ### Examples
///
/// The examples below show the operation on its own, as it would stand inside
/// such a branch:
///
/// ```vpl
/// vector_filter_features expr="name == 'Berlin'"
/// vector_filter_features expr="population >= 1000"
/// vector_filter_features expr="highway in ['primary','secondary']"
/// vector_filter_features expr="name.matches('^St\\.')"
/// vector_filter_features expr="name != null && name != ''"
/// vector_filter_features expr="$props['addr:street'] == 'Hauptstr.'"
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
/// vector_filter_features expr="name == 'Berlin'"
/// ```
///
/// For keys containing `:`, `-`, `.`, or anything else that is not an
/// identifier, use the `$props` map:
///
/// ```vpl
/// vector_filter_features expr="$props['addr:street'] == 'Hauptstr.'"
/// ```
///
/// ### Names starting with `$`
///
/// Three names describe where a feature is rather than what it carries. They
/// start with `$`, which no property name can, so they never collide with one:
///
/// - `$zoom` — the zoom level of the tile being filtered, as an integer.
/// - `$layer` — the name of the feature's layer, as a string.
/// - `$props` — all of the feature's properties, as a map.
///
/// `$zoom` lets one expression keep different features at different zooms —
/// which is how cartography actually reads a layer:
///
/// ```vpl
/// vector_filter_features expr="kind in ['motorway','trunk'] || $zoom >= 12"
/// ```
///
/// That keeps motorways everywhere and every other street only from z12. An
/// expression reading no property is answered once per tile, or once per layer
/// if it reads `$layer`, rather than once per feature, so a plain zoom range is
/// cheap:
///
/// ```vpl
/// vector_filter_features expr="$zoom >= 14"
/// ```
///
/// The bare words `zoom` and `props` are refused, because they meant `$zoom` and
/// `$props` before the `$` was required. A feature property of either name is
/// `$props.zoom` or `$props.props`. So are `_zoom`, `_layer` and `_props`, which
/// are reserved for the same reason. Any other property, including one named
/// `layer`, is a bare identifier as usual.
///
/// ### Missing keys
///
/// A property a feature does not carry resolves to `null` for identifier-safe
/// access, so compare against `null` to say explicitly whether such features are
/// kept or dropped:
///
/// ```vpl
/// vector_filter_features expr="name != null && name != ''"
/// ```
///
/// The `has()` macro asks the same question of an identifier-safe key, and `in`
/// of any key:
///
/// ```vpl
/// vector_filter_features expr="has($props.name)"
/// vector_filter_features expr="'addr:street' in $props"
/// ```
///
/// ### Evaluation failures
///
/// A feature is kept only when the expression evaluates to `true`. An
/// expression that fails to evaluate, or that returns something other than a
/// boolean, drops the feature — there is no separate error channel per feature,
/// so a filter that cannot be answered filters nothing in.
///
/// The common way to reach that is an ordering comparison against a property
/// that is absent or is not a number:
///
/// ```vpl
/// vector_filter_features expr="population >= 1000"
/// ```
///
/// A feature with no `population` compares `null` against a number and is
/// dropped. Guard the comparison to say so deliberately:
///
/// ```vpl
/// vector_filter_features expr="has($props.population) && $props.population >= 1000"
/// ```
///
/// Equality and membership are safe without a guard: `==`, `!=` and `in`
/// compare across types and return `false` rather than failing. Ordering
/// comparisons — `<` `<=` `>` `>=` — are the ones that fail, and a `has()`
/// guard does not rescue a property that is *present* but holds a string,
/// because `&&` short-circuits only on `false`.
///
/// `is_num(x)` closes that gap. It asks whether a value is a number and
/// answers rather than failing, so an ordering comparison can be made total:
///
/// ```vpl
/// vector_filter_features expr="is_num($props.population) && $props.population >= 1000"
/// ```
///
/// This is an extension rather than standard CEL, added because there is no
/// other way to write the guard — `type()` does not exist here, and `int()`
/// and `double()` answer by failing, which costs exactly what the guard was
/// meant to avoid.
///
/// The [CEL language
/// spec](https://github.com/google/cel-spec/blob/master/doc/langdef.md) has the
/// full grammar, built-in functions and string methods.
struct Args {
	/// Boolean CEL expression over the feature's properties.
	expr: CelExpression,
}

#[derive(Debug)]
pub(crate) struct Runner {
	/// The layers the expression applies to, or `None` for every layer. Only
	/// `vector_reduce_to_style` sets one; the operation applies to every layer.
	layer_set: Option<HashSet<String>>,
	expr: FeatureExpr,
}

impl Runner {
	fn from_args(args: &Args) -> Result<Self> {
		Ok(Self {
			layer_set: None,
			expr: FeatureExpr::new(args.expr.as_str())?,
		})
	}

	/// Builds a runner from a layer set and an expression, without going through VPL.
	///
	/// This is what lets `vector_reduce_to_style` apply one predicate per source-layer inside a
	/// single operation. Chaining one `vector_filter_features` per layer would work and would be
	/// the same answer, but each link decodes and re-encodes the whole tile — twenty times over
	/// for a real style, to touch one layer each time.
	pub(crate) fn new(layers: impl IntoIterator<Item = String>, expr: &str) -> Result<Self> {
		Ok(Self {
			layer_set: Some(layers.into_iter().collect()),
			expr: FeatureExpr::new(expr)?,
		})
	}

	fn applies_to(&self, layer_name: &str) -> bool {
		self.layer_set.as_ref().is_none_or(|set| set.contains(layer_name))
	}
}

/// Implemented directly rather than through `AsTileTransform`, because the adapter decodes the
/// tile before the operation is asked anything.
///
/// For an expression that reads only `$zoom` and keeps everything, decoding is the entire cost:
/// measured on a Berlin z13 tile, decode and re-encode is 940 µs of the operation's 988 µs, and
/// the remaining 48 µs is reading the tile. The hoist added in #274 skips the per-feature work but
/// could not skip that, because by then the tile was already decoded. Answering before
/// `into_vector()` lets the whole tile pass through as the bytes it arrived as.
///
/// A zoom range with no predicate is the most common operation a generated reduction pipeline
/// emits, so this is the common path rather than a special case.
impl TileTransform for Runner {
	const TAG: &'static str = "vector_filter_features";

	#[context("Failed to run vector_filter_features")]
	fn run(&self, coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		// An expression reading at most `$zoom` has one answer for the whole tile, and it can be
		// had without looking inside.
		if self.expr.scope() == ExprScope::Tile && self.expr.evaluate(coord, "", &GeoProperties::new()) {
			// Every feature is kept, so there is nothing to rewrite. Hand back the original bytes.
			//
			// The `false` case is not symmetrical: dropping the in-scope layers means knowing
			// which layers the tile has, and that needs the decode this is avoiding.
			return Ok(Some(tile));
		}

		let format = tile.format();
		let Some(vector) = self.filter(coord, tile.into_vector()?)? else {
			return Ok(None);
		};
		Ok(Some(Tile::from_vector(vector, format)?))
	}

	fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
}

impl Runner {
	/// Filters a decoded tile, or returns `Ok(None)` to drop it.
	pub(crate) fn filter(&self, coord: &TileCoord, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		// Still worth asking: a zoom-only expression that answered `false` reaches here, and then
		// every in-scope layer goes rather than being walked feature by feature.
		let scope = self.expr.scope();
		let same_for_the_tile = (scope == ExprScope::Tile).then(|| self.expr.evaluate(coord, "", &GeoProperties::new()));

		// Not `retain_mut`: filtering can fail, and a closure returning `bool` has nowhere to put
		// the error but the floor. A tile whose properties do not decode is reported and dropped
		// by `TransformOp`, rather than quietly losing the features that failed to decode.
		let mut kept = Vec::with_capacity(tile.layers.len());
		for mut layer in tile.layers.drain(..) {
			if !self.applies_to(&layer.name) {
				kept.push(layer);
				continue;
			}
			// An expression reading `$layer` but no property has one answer per layer.
			let same_for_the_layer = same_for_the_tile.or_else(|| {
				(scope == ExprScope::Layer).then(|| self.expr.evaluate(coord, &layer.name, &GeoProperties::new()))
			});
			match same_for_the_layer {
				Some(true) => kept.push(layer),
				Some(false) => (),
				None => {
					let name = layer.name.clone();
					layer.filter_map_properties(|props| {
						if self.expr.evaluate(coord, &name, &props) {
							Some(props)
						} else {
							None
						}
					})?;
					if !layer.features.is_empty() {
						kept.push(layer);
					}
				}
			}
		}
		tile.layers = kept;

		if tile.layers.is_empty() {
			Ok(None)
		} else {
			Ok(Some(tile))
		}
	}
}

impl Runner {
	#[context("Building vector_filter_features operation in VPL node {:?}", vpl_node.name)]
	async fn build_transform(vpl_node: VPLNode, _source: &dyn TileSource, _factory: &PipelineFactory) -> Result<Runner> {
		refuse_removed_parameters(&vpl_node)?;
		let args = Args::from_vpl_node(&vpl_node)?;
		Runner::from_args(&args)
	}
}

crate::operations::macros::define_transform_factory!("vector_filter_features", Args, Runner, requires: Vector, per_tile);

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;
	use versatiles_core::{Blob, TileBBox, TileCompression, TileFormat};
	use versatiles_geometry::{
		geo::{GeoFeature, GeoValue, example_geometry},
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
		run_expr_at(layers, expr, tile, 0)
	}

	/// `run_expr` with the tile placed at a chosen zoom level.
	fn run_expr_at(layers: &[&str], expr: &str, tile: VectorTile, level: u8) -> Result<Option<VectorTile>> {
		let runner = Runner::new(layers.iter().map(|s| (*s).to_string()), expr)?;
		runner.filter(&TileCoord::new(level, 0, 0)?, tile)
	}

	/// Returns `(layer_name, feature_count)` pairs, sorted by layer name.
	fn summarise(tile: &VectorTile) -> Vec<(String, usize)> {
		let mut s: Vec<_> = tile.layers.iter().map(|l| (l.name.clone(), l.features.len())).collect();
		s.sort();
		s
	}

	#[test]
	fn test_args_requires_expr() {
		let n = VPLNode::try_from_str("vector_filter_features").unwrap();
		assert!(Args::from_vpl_node(&n).is_err());

		let n = VPLNode::try_from_str(r#"vector_filter_features expr="true""#).unwrap();
		assert_eq!(Args::from_vpl_node(&n).unwrap().expr.as_str(), "true");
	}

	/// `layer` was replaced by `vector_branch` and `$layer`, and the build says so.
	#[tokio::test]
	async fn test_the_removed_layer_parameter_names_its_replacement() {
		let factory = PipelineFactory::new_dummy();
		let error = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features layer=["debug_x"] expr="true""#)
			.await
			.expect_err("layer= is gone");
		let message = format!("{error:#}");
		assert!(message.contains("no longer has the parameter 'layer'"), "{message}");
		assert!(message.contains("vector_branch"), "{message}");
	}

	/// Without a layer set the expression applies to every layer, which is what
	/// the operation does now.
	#[tokio::test]
	async fn test_the_expression_applies_to_every_layer() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features expr="$layer == 'debug_x'""#)
			.await?;
		let tile = op
			.tile_stream(TileBBox::new_full(0)?)
			.await?
			.next()
			.await
			.unwrap()
			.1
			.into_vector()?;
		let names: Vec<&str> = tile.layers.iter().map(|l| l.name.as_str()).collect();
		assert_eq!(names, ["debug_x"]);
		Ok(())
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
	fn test_undecodable_properties_are_reported_not_swallowed() {
		// The failure used to be discarded at the call site, so a corrupt feature disappeared and
		// the operation reported success. `TransformOp` records the `Err` against the runtime and
		// drops the tile — losing the tile is bad, but losing features without saying so is worse.
		// That `TransformOp` does the recording is covered by `transform::tests`.
		let mut tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("population", GeoValue::Int(2000))]),
				feature(vec![("population", GeoValue::Int(3000))]),
			],
		)]);
		tile.layers[0].features[1].tag_ids = vec![9999, 9999];

		let err = run_expr(&["poi"], "population >= 1000", tile).unwrap_err();
		assert!(
			err.chain()
				.any(|e| e.to_string().contains("Failed to decode properties")),
			"expected the decode failure to name the layer, got: {err:?}"
		);
	}

	#[test]
	fn test_missing_property_is_dropped() {
		// Feature lacks `population`; `name == ...` expression has nothing to match against.
		let tile = VectorTile::new(vec![layer("poi", vec![feature(vec![("other", GeoValue::from("x"))])])]);
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap();
		assert!(out.is_none());
	}

	#[test]
	fn test_zoom_compares_against_an_integer_literal() {
		// `$zoom` is a number the expression can order against, not a string or an opaque value.
		let tile = || VectorTile::new(vec![layer("poi", vec![feature(vec![("a", GeoValue::Int(1))])])]);

		assert_eq!(
			summarise(&run_expr_at(&["poi"], "$zoom >= 12", tile(), 12).unwrap().unwrap()),
			vec![("poi".to_string(), 1)],
			"z12 satisfies $zoom >= 12"
		);
		assert!(
			run_expr_at(&["poi"], "$zoom >= 12", tile(), 5).unwrap().is_none(),
			"z5 does not, and the layer empties"
		);
	}

	#[test]
	fn test_a_zoom_only_keep_never_decodes_the_tile() {
		// The point of implementing `TileTransform` directly: when a zoom-only expression keeps
		// everything, the tile is not decoded at all. Re-encoding a tile that did not change is
		// 940 µs of the operation's 988 µs on a real Berlin z13 tile.
		//
		// Proven with a blob that is not a vector tile. Comparing bytes would not prove it — MVT
		// round-trips byte-identically for a simple tile, so a test written that way passes just
		// as happily when the shortcut is removed. Undecodable input has no such ambiguity: the
		// decoding path must fail on it, so surviving means no decode was attempted.
		let tile = Tile::from_blob(
			Blob::from("not a vector tile"),
			TileCompression::Uncompressed,
			TileFormat::MVT,
		);

		let runner = Runner::new(["streets".to_string()], "$zoom >= 5").unwrap();

		assert!(
			runner.run(&TileCoord::new(9, 0, 0).unwrap(), tile).unwrap().is_some(),
			"a zoom-only keep should pass the tile through without looking inside it"
		);
	}

	#[test]
	fn test_an_expression_reading_properties_does_decode() {
		// The other half of the same proof: the shortcut is not reached when the expression needs
		// to see a feature, so the same undecodable blob is an error rather than a pass-through.
		let tile = Tile::from_blob(
			Blob::from("not a vector tile"),
			TileCompression::Uncompressed,
			TileFormat::MVT,
		);

		let runner = Runner::new(["streets".to_string()], "$props.kind == 'motorway'").unwrap();

		assert!(runner.run(&TileCoord::new(9, 0, 0).unwrap(), tile).is_err());
	}

	#[test]
	fn test_a_zoom_only_drop_still_works_through_the_decoding_path() {
		// The `false` case is not symmetrical: dropping the in-scope layers means knowing which
		// layers the tile has, which needs the decode the keep-case avoids.
		let tile = Tile::from_vector(
			VectorTile::new(vec![layer(
				"streets",
				vec![feature(vec![("kind", GeoValue::from("motorway"))])],
			)]),
			TileFormat::MVT,
		)
		.unwrap();

		let runner = Runner::new(["streets".to_string()], "$zoom >= 5").unwrap();

		assert!(
			runner.run(&TileCoord::new(1, 0, 0).unwrap(), tile).unwrap().is_none(),
			"below the zoom, the only layer goes and the tile with it"
		);
	}

	#[test]
	fn test_zoom_only_expression_keeps_or_drops_whole_layers() {
		// The hoisted path: no property is read, so the tile is not decoded at all. Both features
		// survive together or neither does.
		let tile = || {
			VectorTile::new(vec![layer(
				"streets",
				vec![
					feature(vec![("kind", GeoValue::from("motorway"))]),
					feature(vec![("kind", GeoValue::from("residential"))]),
				],
			)])
		};

		let kept = run_expr_at(&["streets"], "$zoom >= 5 && $zoom <= 14", tile(), 9)
			.unwrap()
			.unwrap();
		assert_eq!(summarise(&kept), vec![("streets".to_string(), 2)]);

		assert!(
			run_expr_at(&["streets"], "$zoom >= 5 && $zoom <= 14", tile(), 15)
				.unwrap()
				.is_none()
		);
	}

	#[test]
	fn test_zoom_combined_with_a_property_predicate() {
		// The shape a generated pipeline emits: keep the important features everywhere, and
		// everything else only from a zoom onwards.
		let tile = || {
			VectorTile::new(vec![layer(
				"streets",
				vec![
					feature(vec![("kind", GeoValue::from("motorway"))]),
					feature(vec![("kind", GeoValue::from("residential"))]),
				],
			)])
		};
		let expr = "kind in ['motorway','trunk'] || $zoom >= 12";

		assert_eq!(
			summarise(&run_expr_at(&["streets"], expr, tile(), 5).unwrap().unwrap()),
			vec![("streets".to_string(), 1)],
			"at z5 only the motorway survives"
		);
		assert_eq!(
			summarise(&run_expr_at(&["streets"], expr, tile(), 12).unwrap().unwrap()),
			vec![("streets".to_string(), 2)],
			"at z12 both do"
		);
	}

	#[test]
	fn test_zoom_and_a_property_named_zoom_do_not_collide() {
		// `$zoom` is the tile's zoom level. A feature property called `zoom` is a different thing,
		// reached through `$props`, and the bare word is refused rather than guessed at.
		let tile = || VectorTile::new(vec![layer("poi", vec![feature(vec![("zoom", GeoValue::Int(3))])])]);

		assert!(
			run_expr_at(&["poi"], "$zoom == 3", tile(), 7).unwrap().is_none(),
			"`$zoom` is the tile's zoom (7), not the property (3)"
		);
		assert_eq!(
			summarise(&run_expr_at(&["poi"], "$props.zoom == 3", tile(), 7).unwrap().unwrap()),
			vec![("poi".to_string(), 1)],
			"the property is reachable through `$props`"
		);
		let error = run_expr_at(&["poi"], "zoom == 3", tile(), 7).unwrap_err();
		assert!(
			format!("{error:#}").contains("`zoom` is written `$zoom` now"),
			"unhelpful message: {error:#}"
		);
	}

	#[test]
	fn test_layer_names_the_layer_being_filtered() {
		let tile = || {
			VectorTile::new(vec![
				layer("poi", vec![feature(vec![("kind", GeoValue::from("shop"))])]),
				layer("place", vec![feature(vec![("kind", GeoValue::from("city"))])]),
			])
		};

		// Answered once per layer: the expression reads no property.
		assert_eq!(
			summarise(
				&run_expr(&["poi", "place"], "$layer == 'place'", tile())
					.unwrap()
					.unwrap()
			),
			vec![("place".to_string(), 1)]
		);
		// Answered per feature, with the layer bound beside the properties.
		assert_eq!(
			summarise(
				&run_expr(&["poi", "place"], "$layer == 'poi' || kind == 'city'", tile())
					.unwrap()
					.unwrap()
			),
			vec![("place".to_string(), 1), ("poi".to_string(), 1)]
		);
	}

	#[test]
	fn test_a_property_named_layer_is_an_ordinary_property() {
		// `layer` was never reserved, so a property of that name, common in OpenMapTiles data,
		// keeps working as a bare identifier.
		let tile = VectorTile::new(vec![layer("roads", vec![feature(vec![("layer", GeoValue::Int(1))])])]);
		assert_eq!(
			summarise(&run_expr(&["roads"], "layer == 1", tile).unwrap().unwrap()),
			vec![("roads".to_string(), 1)]
		);
	}

	#[tokio::test]
	async fn test_zoom_comes_from_the_tile_being_processed() -> Result<()> {
		// End-to-end, so the coordinate arrives from the stream rather than from a test literal:
		// the same pipeline keeps the layer at z2 and drops it at z1.
		let factory = PipelineFactory::new_dummy();
		let vpl = r#"from_debug format=mvt | vector_filter_features expr="$layer != 'debug_x' || $zoom >= 2""#;
		let op = factory.operation_from_vpl(vpl).await?;

		let has_debug_x = async |level: u8| -> Result<bool> {
			let tile = op
				.tile_stream(TileBBox::new_full(level)?)
				.await?
				.next()
				.await
				.unwrap()
				.1
				.into_vector()?;
			Ok(tile.layers.iter().any(|l| l.name == "debug_x"))
		};

		assert!(!has_debug_x(1).await?, "z1 fails $zoom >= 2");
		assert!(has_debug_x(2).await?, "z2 satisfies it");
		Ok(())
	}

	#[test]
	fn test_non_boolean_result_is_dropped() {
		// `evaluate` keeps a feature only on `Ok(Bool(true))`, so an expression that is well-formed
		// but not a predicate drops everything. Pinned because it reads like a bug at the call site
		// and is a deliberate choice: there is no sensible truthiness rule for a tile property.
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![feature(vec![("name", GeoValue::from("Berlin"))])],
		)]);
		let out = run_expr(&["poi"], "name", tile).unwrap();
		assert!(out.is_none(), "a non-boolean expression keeps nothing");
	}

	#[test]
	fn test_type_mismatch_is_dropped() {
		// A property that is a string where the expression assumes a number errors at evaluation
		// time, and an error is indistinguishable from `false` here. This is the failure mode that
		// makes a generated expression need explicit guards — see `test_guarded_comparison_*`.
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("population", GeoValue::from("many"))]),
				feature(vec![("population", GeoValue::Int(2000))]),
			],
		)]);
		let out = run_expr(&["poi"], "population >= 1000", tile).unwrap().unwrap();
		assert_eq!(
			summarise(&out),
			vec![("poi".to_string(), 1)],
			"the string-valued feature errors and is dropped; the int-valued one survives"
		);
	}

	#[test]
	fn test_guarded_comparison_survives_a_missing_property() {
		// CEL's `&&` absorbs an error on one side when the other is `false`, so guarding a
		// comparison with `has()` turns "missing property" from an error into a plain `false`.
		// This is what lets an expression say what should happen to features lacking the key.
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("other", GeoValue::from("x"))]), // no `population` at all
				feature(vec![("population", GeoValue::Int(2000))]),
			],
		)]);
		let out = run_expr(&["poi"], "has($props.population) && $props.population >= 1000", tile)
			.unwrap()
			.unwrap();
		assert_eq!(summarise(&out), vec![("poi".to_string(), 1)]);
	}

	#[test]
	fn test_equality_across_types_is_false_not_an_error() {
		// `==` and `!=` compare across types without erroring — an int is simply not equal to a
		// string. So equality and membership need no type guard, unlike ordering below.
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![
				feature(vec![("population", GeoValue::from("many"))]),
				feature(vec![("population", GeoValue::Int(2000))]),
			],
		)]);
		let out = run_expr(&["poi"], "$props.population != 1000", tile).unwrap().unwrap();
		assert_eq!(
			summarise(&out),
			vec![("poi".to_string(), 2)],
			"both survive: the string is unequal to 1000 rather than incomparable with it"
		);
	}

	#[test]
	fn test_is_num_makes_an_ordering_comparison_total() {
		// The whole point of the extension: with `is_num`, a mistyped value answers `false`
		// instead of erroring, so it is filtered rather than silently lost. Without it, the same
		// tile drops both features — see `test_a_guard_cannot_rescue_an_ordering_type_mismatch`.
		let tile = || {
			VectorTile::new(vec![layer(
				"poi",
				vec![
					feature(vec![("population", GeoValue::from("many"))]),
					feature(vec![("population", GeoValue::Int(2000))]),
					feature(vec![("other", GeoValue::from("x"))]), // no `population` at all
				],
			)])
		};

		let expr = "is_num($props.population) && $props.population >= 1000";
		let out = run_expr(&["poi"], expr, tile()).unwrap().unwrap();
		assert_eq!(
			summarise(&out),
			vec![("poi".to_string(), 1)],
			"only the genuinely-numeric feature passes, and nothing errors"
		);
	}

	#[test]
	fn test_is_num_answers_for_every_value_shape() {
		// It has to be total itself, or it just moves the failure. Each case keeps the feature
		// only if `is_num` returned the expected answer without erroring.
		let cases = [
			(GeoValue::Int(1), true),
			(GeoValue::UInt(1), true),
			(GeoValue::Double(1.5), true),
			(GeoValue::Float(1.5), true),
			(GeoValue::from("text"), false),
			(GeoValue::Bool(true), false),
			(GeoValue::Null, false),
		];

		for (value, expected) in cases {
			let tile = VectorTile::new(vec![layer("poi", vec![feature(vec![("v", value.clone())])])]);
			let kept = run_expr(&["poi"], "is_num($props.v)", tile).unwrap().is_some();
			assert_eq!(kept, expected, "is_num({value:?}) should be {expected}");
		}
	}

	#[test]
	fn test_a_guard_cannot_rescue_an_ordering_type_mismatch() {
		// `has()` only helps when it is false, because `&&` short-circuits before the comparison.
		// A property that is present but of the wrong type still reaches `>=` and still errors,
		// and `cel-interpreter 0.10` offers no `type()` to test for it beforehand.
		//
		// So an ordering comparison cannot be made total in this CEL implementation. Anything
		// generating expressions has to accept that a mistyped value drops its feature, or avoid
		// emitting ordering comparisons at all.
		let tile = VectorTile::new(vec![layer(
			"poi",
			vec![feature(vec![("population", GeoValue::from("many"))])],
		)]);
		let out = run_expr(&["poi"], "has($props.population) && $props.population >= 1000", tile).unwrap();
		assert!(out.is_none(), "the guard does not prevent the comparison from erroring");
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
		// For non-identifier keys (like OSM `addr:street`), `'key' in $props` checks presence.
		let tile = VectorTile::new(vec![layer(
			"addr",
			vec![
				feature(vec![("addr:street", GeoValue::from("Hauptstr."))]),
				feature(vec![("other", GeoValue::from("x"))]),
			],
		)]);
		let out = run_expr(&["addr"], "'addr:street' in $props", tile).unwrap().unwrap();
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
		let out = run_expr(&["poi"], "has($props.name)", tile).unwrap().unwrap();
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
		let out = run_expr(&["addr"], "$props['addr:street'] == 'Hauptstr.'", tile)
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
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features expr="$layer != 'debug_x'""#)
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
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features expr="population >=""#)
			.await;
		assert!(result.is_err(), "invalid CEL should fail at pipeline build time");
	}

	#[tokio::test]
	async fn output_tiles_pass_mvt_validation() -> Result<()> {
		use crate::helpers::assert_tiles_valid;
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(r#"from_debug format=mvt | vector_filter_features expr="true""#)
			.await?;
		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}
}
