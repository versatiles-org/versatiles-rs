//! Turning a requirement into what `vector_reduce_to_style` applies to a tile: the layers to keep
//! and one CEL feature filter per layer that needs one.
//!
//! The expressions are the ones `vector_filter_features` evaluates, so there is a single
//! implementation of what an expression means. Properties need no rendering: the operation reads
//! [`Properties`](super::Properties) per tile zoom directly, which no regex over `layer/property`
//! could express.

use super::{
	ir::{KeepEntry, Requirement},
	predicate,
};

/// The source-layers to keep. Everything else is dropped.
#[must_use]
pub fn layer_names(requirement: &Requirement) -> Vec<String> {
	requirement.layers.keys().cloned().collect()
}

/// One feature filter per layer that needs one.
///
/// A layer whose entries add up to "keep everything" gets no operation at all, rather than one
/// with `expr="true"`: the operation would decode and re-encode every feature's properties to
/// arrive back where it started.
#[must_use]
pub fn feature_expressions(requirement: &Requirement) -> Vec<(String, String)> {
	let mut nodes = Vec::new();

	for (layer, requirements) in &requirement.layers {
		// An empty `keep` says no feature of this layer is ever drawn. Read as written that means
		// dropping the layer's features entirely — but an over-approximating format is far more
		// likely to have failed to describe the layer than to mean "none of it", and the contract
		// says to err toward keeping. So it widens, like every other gap.
		if requirements.keep.is_empty() {
			continue;
		}

		let terms: Vec<String> = requirements.keep.iter().map(entry).collect();

		// Any entry that keeps everything makes the whole disjunction keep everything.
		if terms.iter().any(|term| term == "true") {
			continue;
		}

		// Parenthesise only when there is something to disambiguate. A single entry is already
		// the whole expression, and `((zoom >= 14 && zoom <= 14))` is noise in a pipeline someone
		// is expected to read and edit.
		let expression = if terms.len() == 1 {
			terms.into_iter().next().unwrap()
		} else {
			terms.iter().map(|t| format!("({t})")).collect::<Vec<_>>().join(" || ")
		};

		nodes.push((layer.clone(), expression));
	}

	nodes
}

/// One `keep` entry: its zoom range and its predicate, both optional.
fn entry(entry: &KeepEntry) -> String {
	let mut terms: Vec<String> = Vec::new();

	if let Some(min) = entry.minzoom {
		terms.push(format!("zoom >= {min}"));
	}
	if let Some(max) = entry.maxzoom {
		terms.push(format!("zoom <= {max}"));
	}

	if let Some(predicate) = &entry.predicate {
		// Starts positive: an entry sits in a disjunction of things to keep.
		let rendered = predicate::render(predicate, true);
		// A predicate that widened to "keep anything" adds nothing but noise to the conjunction.
		if rendered != "true" {
			terms.push(rendered);
		}
	}

	match terms.len() {
		// No zoom bounds and no predicate: this entry keeps everything, at every zoom.
		0 => "true".to_string(),
		1 => terms.into_iter().next().unwrap(),
		// No outer parentheses: the caller adds them if it joins this with another entry.
		_ => terms.join(" && "),
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::{
		super::{LayerRequirement, Properties, style::from_style},
		*,
	};

	/// One of the styles vendored from `tiles.versatiles.org/assets/styles/`.
	fn fixture(name: &str) -> Requirement {
		let path = format!("{}/../testdata/styles/{name}", env!("CARGO_MANIFEST_DIR"));
		let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
		from_style(&json, 14, &[], &std::collections::BTreeSet::new()).unwrap()
	}

	/// An inline style, analysed against a z14 tileset.
	fn parse(json: &str) -> Requirement {
		from_style(json, 14, &[], &std::collections::BTreeSet::new()).unwrap()
	}

	/// The feature expressions, as `layer: expression` lines.
	fn expressions(requirement: &Requirement) -> Vec<String> {
		feature_expressions(requirement)
			.into_iter()
			.map(|(layer, expression)| format!("{layer}: {expression}"))
			.collect()
	}

	/// The `streets`/`buildings` shape the format was documented with, as a style.
	const STREETS: &str = r#"{"layers":[
		{"id":"streets-major","source-layer":"streets","minzoom":5,
		 "filter":["all",["in",["get","kind"],["literal",["motorway","trunk"]]],["!=",["get","bridge"],true]],
		 "layout":{"text-field":["get","tunnel"]},
		 "paint":{"line-width":["get","service"]}},
		{"id":"streets-all","source-layer":"streets","minzoom":12},
		{"id":"buildings","source-layer":"buildings","minzoom":14}
	]}"#;

	#[test]
	fn renders_the_documentation_example() {
		let r = parse(STREETS);
		assert_eq!(layer_names(&r), ["buildings", "streets"]);
		assert_eq!(
			expressions(&r),
			[
				"buildings: zoom >= 14",
				"streets: (zoom >= 5 && ((has(props.kind) && props.kind in ['motorway', 'trunk']) && !((has(props.bridge) && props.bridge == true)))) || (zoom >= 12)",
			]
		);
	}

	#[test]
	fn a_layer_that_keeps_everything_gets_no_feature_filter() {
		// Unfiltered and drawn at every zoom, so the filter would decode every feature's properties
		// to arrive back where it started.
		let r = parse(r#"{"layers":[{"id":"a","source-layer":"a","paint":{"fill-color":["get","k"]}}]}"#);
		assert_eq!(expressions(&r), Vec::<String>::new());
	}

	#[test]
	fn one_unbounded_entry_makes_the_whole_disjunction_unbounded() {
		// `keep` is an OR, so an entry that matches everything subsumes the others. Here a second
		// style layer draws the same source-layer at every zoom with no filter.
		let r = parse(
			r#"{"layers":[
				{"id":"a","source-layer":"a","minzoom":5,"filter":["has","k"]},
				{"id":"b","source-layer":"a"}
			]}"#,
		);
		assert_eq!(expressions(&r), Vec::<String>::new());
	}

	#[test]
	fn an_empty_keep_widens_rather_than_dropping_the_layer() {
		// The analysis cannot produce this — a source-layer exists in the requirement only because
		// some style layer drew it — but read literally an empty disjunction matches nothing, and
		// the guard that reads it as "keep" instead is what the contract rests on. Built directly,
		// since no style can express it.
		let r = Requirement {
			layers: [(
				"a".to_string(),
				LayerRequirement {
					properties: Properties::Listed([("k".to_string(), 0..=14)].into_iter().collect()),
					keep: Vec::new(),
				},
			)]
			.into_iter()
			.collect(),
		};
		assert_eq!(expressions(&r), Vec::<String>::new());
		assert_eq!(layer_names(&r), ["a"], "the layer survives");
	}

	#[test]
	fn a_widened_predicate_leaves_only_its_zoom_range() {
		// An unreadable filter contributes nothing, so the entry is its zoom test alone — which
		// `vector_filter_features` then answers once per tile rather than once per feature.
		let r = parse(r#"{"layers":[{"id":"a","source-layer":"a","minzoom":12,"filter":["sorcery","k"]}]}"#);
		assert_eq!(expressions(&r), ["a: zoom >= 12"]);
	}

	/// What `check` makes of every expression, as a `vector_filter_features` node.
	///
	/// `check_pipeline` needs no factory, no runtime and no tiles, but it compiles the CEL with the
	/// same code `vector_reduce_to_style` does. A mis-escaped name or an expression with unbalanced
	/// parentheses is caught here rather than as a pipeline that refuses to build. The nodes are built
	/// as values, so VPL quoting is the serializer's problem.
	fn problems(requirement: &Requirement) -> Vec<String> {
		let mut nodes = vec!["from_debug format=mvt".to_string()];
		for (layer, expression) in feature_expressions(requirement) {
			let node = crate::vpl::VPLNode {
				name: "vector_filter_features".to_string(),
				properties: [
					("layer".to_string(), vec![layer]),
					("expr".to_string(), vec![expression]),
				]
				.into_iter()
				.collect(),
				sources: Vec::new(),
			};
			nodes.push(node.to_string());
		}
		let text = nodes.join(" | ");
		let pipeline =
			crate::vpl::parse_vpl(&text).unwrap_or_else(|e| panic!("rendered VPL does not parse: {text}\n{e}"));
		crate::check_pipeline(&pipeline)
			.into_iter()
			.map(|p| p.message)
			.collect()
	}

	#[test]
	fn every_deployed_style_renders_expressions_that_compile() {
		// The real cartography, not a fixture written to suit the renderer: 324 style layers and
		// 318 filters in `colorful`, 207 and 203 in `neutrino`.
		for name in ["colorful.json", "neutrino.json"] {
			assert_eq!(problems(&fixture(name)), Vec::<String>::new(), "{name} rendered badly");
		}
	}

	#[test]
	fn awkward_names_still_render_expressions_that_compile() {
		// Every character class that has bitten an escaper: quotes, backslashes, regex
		// metacharacters, spaces, and a CEL keyword as a property name.
		let r = parse(
			r#"{"layers":[
				{"id":"x","source-layer":"it's a layer","minzoom":3,
				 "filter":["==","quote'd","a'b"],
				 "paint":{"a":["get","addr:street"],"b":["get","name.en"],"c":["get","a\\b"],"d":["get","in"]}},
				{"id":"y","source-layer":"plain",
				 "filter":["in","a\\b","x|y","^z$"],
				 "paint":{"a":["get","k"]}}
			]}"#,
		);
		assert_eq!(problems(&r), Vec::<String>::new(), "got: {:?}", expressions(&r));
	}
}
