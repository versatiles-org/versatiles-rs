//! Turning a requirement into the VPL operations that enforce it.
//!
//! The output is a list of transform nodes, not a whole pipeline: what they are applied to is the
//! caller's business, and keeping the source out of it is what makes this a pure function of the
//! requirement — testable as golden text, and callable by an editor that wants the operations to
//! append to a pipeline someone is already writing.
//!
//! Nodes are built as [`VPLNode`] values rather than assembled as text. VPL quoting already
//! exists, is already tested, and picks the least punctuation that parses back; a second
//! implementation here would be a place for a mis-escaped layer name to hide.

use std::collections::BTreeMap;

use super::{
	ir::{KeepEntry, Requirement},
	predicate,
};
use crate::vpl::VPLNode;

/// Builds one transform node.
fn node(name: &str, properties: &[(&str, Vec<String>)]) -> VPLNode {
	VPLNode {
		name: name.to_string(),
		properties: properties
			.iter()
			.map(|(key, values)| ((*key).to_string(), values.clone()))
			.collect::<BTreeMap<_, _>>(),
		sources: Vec::new(),
	}
}

/// The operations that reduce a tileset to `requirement`.
///
/// Order matters and is not a preference: **properties are stripped last**, because the feature
/// filter's expressions read properties, and removing them first would make every `has(props.k)`
/// guard false and delete the features the filter was meant to keep.
#[must_use]
pub fn operations(requirement: &Requirement) -> Vec<VPLNode> {
	let mut nodes = Vec::new();

	nodes.push(layer_filter(requirement));
	nodes.extend(feature_filters(requirement));
	nodes.push(property_filter(requirement));

	nodes
}

/// Keeps only the layers the requirement names.
fn layer_filter(requirement: &Requirement) -> VPLNode {
	let layers: Vec<String> = requirement.layers.keys().cloned().collect();
	node(
		"vector_filter_layers",
		&[("filter", layers), ("invert", vec!["true".to_string()])],
	)
}

/// Keeps only the properties the requirement names, across every layer.
///
/// One regex over `layer/property`, which is what `vector_filter_properties` matches against. A
/// layer whose `properties` list is empty contributes no alternative and so loses every property
/// while keeping its geometry — the case where a layer is drawn as shape alone.
fn property_filter(requirement: &Requirement) -> VPLNode {
	let mut alternatives: Vec<String> = Vec::new();
	for (layer, requirements) in &requirement.layers {
		for property in &requirements.properties {
			alternatives.push(regex::escape(&format!("{layer}/{property}")));
		}
	}

	// An empty alternation is not the empty pattern: `^(?:)$` matches the empty string rather
	// than nothing at all. `[^\s\S]` asks for one character that is neither whitespace nor
	// non-whitespace, so it can never match — and unlike `(?!)`, it needs no look-around, which
	// the `regex` crate does not support.
	let pattern = if alternatives.is_empty() {
		r"[^\s\S]".to_string()
	} else {
		format!("^(?:{})$", alternatives.join("|"))
	};

	node(
		"vector_filter_properties",
		&[("regex", vec![pattern]), ("invert", vec!["true".to_string()])],
	)
}

/// One feature filter per layer that needs one.
///
/// A layer whose entries add up to "keep everything" gets no operation at all, rather than one
/// with `expr="true"`: the operation would decode and re-encode every feature's properties to
/// arrive back where it started.
fn feature_filters(requirement: &Requirement) -> Vec<VPLNode> {
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

		nodes.push(node(
			"vector_filter_features",
			&[("layer", vec![layer.clone()]), ("expr", vec![expression])],
		));
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
		super::{ir::Requirement, style::from_style},
		*,
	};

	/// One of the styles vendored from `tiles.versatiles.org/assets/styles/`.
	fn fixture(name: &str) -> Requirement {
		let path = format!("{}/../testdata/styles/{name}", env!("CARGO_MANIFEST_DIR"));
		let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
		from_style(&json, 14).unwrap()
	}

	/// The rendered operations as VPL text, one per line.
	fn render_text(requirement: &Requirement) -> String {
		operations(requirement)
			.iter()
			.map(ToString::to_string)
			.collect::<Vec<_>>()
			.join("\n")
	}

	/// An inline style, analysed against a z14 tileset.
	fn parse(json: &str) -> Requirement {
		from_style(json, 14).unwrap()
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
		// Parameters come out in alphabetical order and quoting picks the least punctuation that
		// parses back — both the serializer's doing, which is the point of building nodes rather
		// than text.
		assert_eq!(
			render_text(&parse(STREETS)),
			[
				r"vector_filter_layers filter=[buildings, streets] invert=true",
				r"vector_filter_features expr='zoom >= 14' layer=buildings",
				r#"vector_filter_features expr="(zoom >= 5 && ((has(props.kind) && props.kind in ['motorway', 'trunk']) && !((has(props.bridge) && props.bridge == true)))) || (zoom >= 12)" layer=streets"#,
				r"vector_filter_properties invert=true regex='^(?:streets/bridge|streets/kind|streets/service|streets/tunnel)$'",
			]
			.join("\n")
		);
	}

	#[test]
	fn buildings_is_drawn_as_geometry_so_keeps_no_properties() {
		// `buildings` is drawn with no filter and nothing read from it, so it contributes nothing to
		// the property regex and gets a zoom-only feature filter.
		let text = render_text(&parse(STREETS));
		assert!(
			!text.contains("buildings/"),
			"no property of buildings survives: {text}"
		);
		assert!(
			text.contains(r"vector_filter_features expr='zoom >= 14' layer=buildings"),
			"buildings keeps a zoom-only filter: {text}"
		);
	}

	#[test]
	fn the_property_filter_comes_last() {
		// Not cosmetic: the feature filters read properties through `has(props.k)`, so stripping
		// properties first would make every guard false and delete the features they keep.
		let names: Vec<String> = operations(&parse(STREETS)).iter().map(|n| n.name.clone()).collect();
		assert_eq!(names.first().unwrap(), "vector_filter_layers");
		assert_eq!(names.last().unwrap(), "vector_filter_properties");
	}

	#[test]
	fn a_layer_that_keeps_everything_gets_no_feature_filter() {
		// Unfiltered and drawn at every zoom, so the operation would decode and re-encode every
		// feature to arrive back where it started.
		let r = parse(r#"{"layers":[{"id":"a","source-layer":"a","paint":{"fill-color":["get","k"]}}]}"#);
		let text = render_text(&r);
		assert!(!text.contains("vector_filter_features"), "got: {text}");
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
		assert!(!render_text(&r).contains("vector_filter_features"));
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
				crate::reduce::LayerRequirement {
					properties: vec!["k".to_string()],
					keep: Vec::new(),
				},
			)]
			.into_iter()
			.collect(),
		};
		let text = render_text(&r);
		assert!(!text.contains("vector_filter_features"), "got: {text}");
		assert!(
			text.contains("vector_filter_layers filter=a"),
			"the layer survives: {text}"
		);
	}

	#[test]
	fn a_widened_predicate_leaves_only_its_zoom_range() {
		// An unreadable filter contributes nothing, so the entry is its zoom test alone — which
		// `vector_filter_features` then answers once per tile rather than once per feature.
		let r = parse(r#"{"layers":[{"id":"a","source-layer":"a","minzoom":12,"filter":["sorcery","k"]}]}"#);
		assert!(
			render_text(&r).contains(r"vector_filter_features expr='zoom >= 12' layer=a"),
			"got: {}",
			render_text(&r)
		);
	}

	#[test]
	fn property_names_are_escaped_into_the_regex() {
		// `addr:street` and `name.en` carry regex metacharacters; unescaped, `.` would match any
		// character and keep properties the style never asked for.
		let r = parse(
			r#"{"layers":[{"id":"a","source-layer":"a",
				"layout":{"text-field":["get","name.en"]},"paint":{"x":["get","addr:street"]}}]}"#,
		);
		let text = render_text(&r);
		assert!(text.contains(r"a/name\.en"), "the dot is escaped: {text}");
	}

	#[test]
	fn a_requirement_that_reads_no_properties_keeps_none() {
		// `[^\s\S]` never matches, so with `invert=true` every property is dropped. An empty
		// alternation would have been `^(?:)$`, which matches the empty string instead.
		let r = parse(r##"{"layers":[{"id":"a","source-layer":"a","paint":{"fill-color":"#fff"}}]}"##);
		assert!(render_text(&r).contains(r"regex='[^\s\S]'"), "got: {}", render_text(&r));
	}

	/// Renders `requirement` into a full pipeline and reports what `check` makes of it.
	///
	/// This is the cheap half of proving the renderer right: `check_pipeline` needs no factory,
	/// no runtime and no tiles, but it resolves every operation name, validates every parameter
	/// against the operation's own metadata, and compiles the CEL. A mis-escaped name or an
	/// expression with unbalanced parentheses is caught here rather than as a silently wrong
	/// reduction later.
	fn problems(requirement: &Requirement) -> Vec<String> {
		let text = format!(
			"from_debug format=mvt | {}",
			render_text(requirement).replace('\n', " | ")
		);
		let pipeline =
			crate::vpl::parse_vpl(&text).unwrap_or_else(|e| panic!("rendered VPL does not parse: {text}\n{e}"));
		crate::check_pipeline(&pipeline)
			.into_iter()
			.map(|p| p.message)
			.collect()
	}

	#[test]
	fn every_deployed_style_renders_a_valid_pipeline() {
		// The real cartography, not a fixture written to suit the renderer: 324 style layers and
		// 318 filters in `colorful`, 207 and 203 in `neutrino`.
		for name in ["colorful.json", "neutrino.json"] {
			assert_eq!(problems(&fixture(name)), Vec::<String>::new(), "{name} rendered badly");
		}
	}

	#[test]
	fn awkward_names_still_render_a_valid_pipeline() {
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
		assert_eq!(problems(&r), Vec::<String>::new(), "got: {}", render_text(&r));
	}

	#[test]
	fn layer_names_needing_quotes_survive_the_serializer() {
		// Building nodes rather than text means VPL quoting is the serializer's problem, and it
		// already knows that a name with a space cannot be bare.
		let r = parse(r#"{"layers":[{"id":"a","source-layer":"my layer","minzoom":3,"paint":{"a":["get","k"]}}]}"#);
		let text = render_text(&r);
		assert!(text.contains("filter='my layer'"), "got: {text}");
		assert!(text.contains("layer='my layer'"), "got: {text}");
	}
}
