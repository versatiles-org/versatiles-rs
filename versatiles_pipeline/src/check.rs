//! Static validation of a VPL pipeline, without building it.
//!
//! Building a pipeline opens files and hits the network, so it cannot run on a
//! keystroke — and it conflates two different failures. `from_container
//! filename=missing.mbtiles` is the *world* being wrong; `from_debug
//! format=png nonsense=1` is the *pipeline* being wrong, and an editor should
//! be able to say so before anything is opened.
//!
//! [`PipelineFactory::check`](crate::PipelineFactory::check) reports the second
//! kind only. It performs no I/O.
//!
//! Values are checked as far as the metadata allows: a parameter with an
//! enumerated type is handed to that type's own parser
//! ([`VPLFieldMeta::accepts`](crate::vpl::VPLFieldMeta::accepts)), so
//! `format=notaformat` is reported and the alias `format=pbf` is not. A value
//! that is merely the wrong *format* for an unenumerated type — `color=red` is
//! not hex — still needs building.

use std::collections::HashMap;

use crate::{
	PipelineFactory,
	operations::{read_operation_factories, transform_operation_factories},
	vpl::{VPLFieldMeta, VPLNode, VPLPipeline},
};

/// Parameter metadata for every operation of one kind, by name.
type Registry = HashMap<String, Vec<VPLFieldMeta>>;

/// Checks a pipeline against the built-in operations without building it.
///
/// The free-standing counterpart to [`PipelineFactory::check`], for callers that
/// have no factory — an editor, a CI check, a `--dry-run`. It needs no runtime
/// and no reader callback, so it cannot perform I/O even by accident.
#[must_use]
pub fn check_pipeline(pipeline: &VPLPipeline) -> Vec<VplProblem> {
	let read = read_operation_factories()
		.iter()
		.map(|f| (f.tag_name().to_string(), f.field_metadata()))
		.collect::<Registry>();
	let transform = transform_operation_factories()
		.iter()
		.map(|f| (f.tag_name().to_string(), f.field_metadata()))
		.collect::<Registry>();

	check_against(pipeline, &read, &transform)
}

fn check_against(pipeline: &VPLPipeline, read: &Registry, transform: &Registry) -> Vec<VplProblem> {
	let mut problems = Vec::new();
	check_pipeline_at(pipeline, read, transform, &mut Vec::new(), &mut problems);
	problems
}

/// Something wrong with a pipeline, found without building it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VplProblem {
	/// Human-readable description, phrased for the person who wrote the VPL.
	pub message: String,
	/// Which node, as a path of indices from the root pipeline.
	///
	/// `[2]` is the third node of the pipeline. Descending into a node's
	/// bracketed sources appends the source index and then the node index, so
	/// `[0, 1, 2]` is the third node of the second source of the first node.
	/// The same walk applies to a [`CstPipeline`](crate::vpl::CstPipeline), which
	/// is how a caller turns this back into a span.
	pub path: Vec<usize>,
	/// The parameter at fault, when the problem is about one.
	///
	/// Pairs with [`CstNode::property`](crate::vpl::CstNode::property) to locate
	/// the exact token.
	pub property: Option<String>,
}

impl VplProblem {
	fn new(path: &[usize], message: impl Into<String>) -> Self {
		VplProblem {
			message: message.into(),
			path: path.to_vec(),
			property: None,
		}
	}

	fn about(path: &[usize], property: &str, message: impl Into<String>) -> Self {
		VplProblem {
			message: message.into(),
			path: path.to_vec(),
			property: Some(property.to_string()),
		}
	}
}

impl PipelineFactory {
	/// Checks a pipeline against the operations registered on *this* factory,
	/// without building it.
	///
	/// Reports everything it can find rather than stopping at the first problem,
	/// so an editor can underline all of them at once.
	///
	/// What it decides about a parameter is its *shape*: how many values it
	/// takes, whether each one parses as the field's numeric type, and — for
	/// types with a `TryFrom<&str>` parser — whether that parser accepts it.
	/// Meaning the Rust type does not carry is out of reach: `epsg=99999` is a
	/// perfectly good `u32` and not an EPSG code. Closing that means giving the
	/// field a type that parses (#257), the way `color` is a `HexColor` rather
	/// than a `String` — not giving `check` a second opinion about values.
	///
	/// So an empty result means nothing is wrong *with the pipeline* — building
	/// it can still fail because a file is missing, or because a value has the
	/// right shape and the wrong meaning.
	///
	/// Never performs I/O. Use [`check_pipeline`] when there is no factory to
	/// hand and the built-in operations are enough.
	#[must_use]
	pub fn check(&self, pipeline: &VPLPipeline) -> Vec<VplProblem> {
		check_against(pipeline, &self.read_registry(), &self.transform_registry())
	}
}

fn check_pipeline_at(
	pipeline: &VPLPipeline,
	read: &Registry,
	transform: &Registry,
	path: &mut Vec<usize>,
	problems: &mut Vec<VplProblem>,
) {
	if pipeline.pipeline.is_empty() {
		problems.push(VplProblem::new(path, "pipeline is empty"));
		return;
	}

	for (index, node) in pipeline.pipeline.iter().enumerate() {
		path.push(index);
		// The head reads; everything after it transforms.
		check_node(node, index == 0, read, transform, path, problems);
		path.pop();
	}
}

fn check_node(
	node: &VPLNode,
	is_head: bool,
	read: &Registry,
	transform: &Registry,
	path: &mut Vec<usize>,
	problems: &mut Vec<VplProblem>,
) {
	let name = node.name.as_str();
	let is_read = read.contains_key(name);
	let is_transform = transform.contains_key(name);

	// Position first: an operation used in the wrong place is a different
	// mistake from one that does not exist, and saying so is more useful than
	// "unknown operation".
	let fields = match (is_head, is_read, is_transform) {
		(true, true, _) => read.get(name),
		(false, _, true) => transform.get(name),
		(true, false, true) => {
			problems.push(VplProblem::new(
				path,
				format!("'{name}' is a transform operation, but a pipeline has to start with a read operation"),
			));
			None
		}
		(false, true, false) => {
			problems.push(VplProblem::new(
				path,
				format!("'{name}' is a read operation, so it can only be the first node of a pipeline"),
			));
			None
		}
		_ => {
			problems.push(VplProblem::new(path, format!("unknown operation '{name}'")));
			None
		}
	};

	if let Some(fields) = fields {
		check_properties(node, fields, path, problems);
		check_sources(node, fields, path, problems);
	}

	// Nested pipelines are checked whether or not the parent was understood.
	for (source_index, source) in node.sources.iter().enumerate() {
		path.push(source_index);
		check_pipeline_at(source, read, transform, path, problems);
		path.pop();
	}
}

fn check_properties(node: &VPLNode, fields: &[VPLFieldMeta], path: &[usize], problems: &mut Vec<VplProblem>) {
	for (key, values) in &node.properties {
		let Some(field) = fields.iter().find(|f| &f.name == key) else {
			let known = fields
				.iter()
				.filter(|f| !f.is_sources)
				.map(|f| f.name.as_str())
				.collect::<Vec<_>>();
			problems.push(VplProblem::about(
				path,
				key,
				format!(
					"'{}' has no parameter '{key}'. Available: {}",
					node.name,
					known.join(", ")
				),
			));
			continue;
		};

		// What counts as a problem is the derive's business, not this
		// function's: it emits probes that call the same parsers the accessors
		// call, so anything reported here is something building would refuse.
		// All that is left to do is name the operation the problem belongs to.
		let Some(validate) = field.validate else {
			continue;
		};
		for reason in validate(values) {
			problems.push(VplProblem::about(path, key, format!("'{}' {reason}", node.name)));
		}
	}

	for field in fields {
		if field.is_required && !field.is_sources && !node.properties.contains_key(&field.name) {
			problems.push(VplProblem::about(
				path,
				&field.name,
				format!("'{}' requires the parameter '{}'", node.name, field.name),
			));
		}
	}
}

fn check_sources(node: &VPLNode, fields: &[VPLFieldMeta], path: &[usize], problems: &mut Vec<VplProblem>) {
	let Some(sources) = fields.iter().find(|f| f.is_sources) else {
		if !node.sources.is_empty() {
			problems.push(VplProblem::new(path, format!("'{}' does not take sources", node.name)));
		}
		return;
	};

	if sources.is_required && node.sources.is_empty() {
		problems.push(VplProblem::new(
			path,
			format!("'{}' needs at least one source", node.name),
		));
	}
}

#[cfg(test)]
mod tests {
	use anyhow::Result;

	use crate::{PipelineFactory, vpl::parse_vpl};

	fn problems(vpl: &str) -> Vec<String> {
		let factory = PipelineFactory::new_dummy();
		factory
			.check(&parse_vpl(vpl).unwrap())
			.into_iter()
			.map(|p| p.message)
			.collect()
	}

	#[test]
	fn a_valid_pipeline_has_no_problems() {
		assert!(problems("from_debug format=png | filter level_max=5").is_empty());
		assert!(problems("from_stacked [ from_debug format=png, from_debug format=png ]").is_empty());
	}

	#[test]
	fn unknown_operation() {
		assert_eq!(problems("from_nonsense"), ["unknown operation 'from_nonsense'"]);
	}

	#[test]
	fn unknown_parameter() {
		let found = problems("from_debug format=png nonsense=1");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(
			found[0].starts_with("'from_debug' has no parameter 'nonsense'. Available: "),
			"{found:?}"
		);
	}

	#[test]
	fn missing_required_parameter() {
		assert_eq!(
			problems("from_container"),
			["'from_container' requires the parameter 'filename'"]
		);
	}

	#[test]
	fn invalid_enum_values_are_reported() {
		let found = problems("from_debug format=nonsense");
		assert_eq!(found.len(), 1, "{found:?}");
		assert!(
			found[0].starts_with("'from_debug' does not accept 'format=nonsense'. Values: "),
			"{found:?}"
		);
	}

	/// The reason this is checked with the parser rather than against
	/// `enum_variants`: the list is the canonical names, and the parsers take
	/// aliases besides. Reporting one would reject VPL that builds, which is
	/// worse than reporting nothing.
	#[test]
	fn accepted_aliases_are_never_reported() {
		assert!(problems("from_debug format=pbf").is_empty());
		assert!(problems("from_debug format=jpeg").is_empty());
		// The parsers normalise case and surrounding space, so `check` does too.
		assert!(problems("from_debug format=PNG").is_empty());
	}

	/// `MaxTileBytes` parses through `TryFrom<&str>` without a closed variant
	/// list, so it is checked but has no values to suggest.
	#[test]
	fn a_type_with_no_variant_list_is_still_checked() {
		assert_eq!(
			problems("from_geo filename=a.geojson max_tile_bytes=fnord"),
			[
				"'from_geo' does not accept 'max_tile_bytes=fnord': invalid max_tile_bytes 'fnord'; \
				 expected a byte count (e.g. 2097152) or 'none' to disable the cap"
			]
		);
		assert!(problems("from_geo filename=a.geojson max_tile_bytes=none").is_empty());
		assert!(problems("from_geo filename=a.geojson max_tile_bytes=4096").is_empty());
	}

	/// Every value of a list is judged, not just the first.
	#[test]
	fn each_value_of_a_repeated_parameter_is_checked() {
		let found = problems("from_debug format=png | raster_flatten color=[a,b,c]");
		assert_eq!(found.len(), 3, "{found:?}");
		assert!(found[2].contains("'color=c'"), "{found:?}");
	}

	/// A scalar parameter given a list is wrong twice over, and both are worth
	/// saying: the value that does not parse, and the count. `get_property`
	/// refuses more than one value, so the builder rejects this either way.
	#[test]
	fn a_bad_value_and_a_bad_count_are_both_reported() {
		let found = problems("from_debug format=[png, nonsense]");
		assert_eq!(found.len(), 2, "{found:?}");
		assert!(found[0].starts_with("'from_debug' does not accept 'format=nonsense'."), "{found:?}");
		assert_eq!(found[1], "'from_debug' expects a single value for 'format', got 2");
	}

	/// The shape of a value is decidable from the metadata even where its
	/// meaning is not: `epsg` is checked as a number without anyone claiming to
	/// know which numbers are EPSG codes.
	#[test]
	fn a_value_that_is_not_a_number_is_reported() {
		assert_eq!(
			problems("from_debug format=png | filter level_max=abc"),
			["'filter' does not accept 'level_max=abc': invalid digit found in string"]
		);
	}

	/// The parser's own error, not a guess at one: 300 is a perfectly good
	/// number and a bad `u8`, and only `parse` knows which of those matters.
	#[test]
	fn a_number_out_of_range_is_reported() {
		assert_eq!(
			problems("from_debug format=png | filter level_max=300"),
			["'filter' does not accept 'level_max=300': number too large to fit in target type"]
		);
	}

	#[test]
	fn an_array_of_the_wrong_length_is_reported() {
		assert_eq!(
			problems("from_debug format=png | raster_flatten color=[0,0]"),
			["'raster_flatten' expects 3 values for 'color', got 2"]
		);
	}

	/// The case #255 is about, with the corners transposed rather than the CRS
	/// wrong — and unlike the CRS, decidable from the literal alone.
	///
	/// A `GeoBBox` is judged as a group because that is the only way to see it:
	/// every one of these four numbers is a fine `f64` on its own, and it is the
	/// relationship between them that is wrong.
	#[test]
	fn a_bounding_box_is_judged_as_a_group() {
		assert_eq!(
			problems("from_debug format=png | filter bbox=[13.5,52.6,13.3,52.4]"),
			["'filter' does not accept 'bbox=[13.5,52.6,13.3,52.4]': x_min (13.5) must be <= x_max (13.3)"]
		);
		assert_eq!(
			problems("from_debug format=png | filter bbox=[0,0,200,10]"),
			["'filter' does not accept 'bbox=[0,0,200,10]': x_max (200) must be <= 180"]
		);
		assert_eq!(
			problems("from_debug format=png | filter bbox=[0,0,10]"),
			["'filter' does not accept 'bbox=[0,0,10]': expected 4 values [west, south, east, north], got 3"]
		);
		assert!(problems("from_debug format=png | filter bbox=[13.3,52.4,13.5,52.6]").is_empty());
	}

	/// A list-typed parameter takes any number of values and judges none of
	/// them, so it carries no validator at all.
	#[test]
	fn a_string_list_takes_any_number_of_values() {
		assert!(problems("from_debug format=png | vector_filter_features layer=[a,b,c] expr=true").is_empty());
	}

	#[test]
	fn composite_without_sources() {
		assert_eq!(problems("from_stacked"), ["'from_stacked' needs at least one source"]);
	}

	#[test]
	fn operation_in_the_wrong_position() {
		assert_eq!(
			problems("filter level_max=5"),
			["'filter' is a transform operation, but a pipeline has to start with a read operation"]
		);
		assert_eq!(
			problems("from_debug format=png | from_debug format=png"),
			["'from_debug' is a read operation, so it can only be the first node of a pipeline"]
		);
	}

	#[test]
	fn problems_are_reported_for_nested_pipelines_with_a_path() {
		let factory = PipelineFactory::new_dummy();
		let found = factory
			.check(&parse_vpl("from_stacked [ from_debug format=png, from_debug nonsense=1 ]").unwrap())
			.into_iter()
			.map(|p| (p.path, p.property))
			.collect::<Vec<_>>();

		// First node of the root, its second source, that source's first node.
		assert_eq!(found, [(vec![0, 1, 0], Some("nonsense".to_string()))]);
	}

	#[test]
	fn every_problem_is_reported_not_just_the_first() {
		assert_eq!(problems("from_debug nonsense=1 | filter rubbish=2").len(), 2);
	}

	/// The rule this whole feature exists to keep: `check` must not reject
	/// anything the builder accepts. Studio maintained this as a differential
	/// test against `operation_from_vpl` because there was no API; it belongs
	/// here.
	#[tokio::test]
	async fn check_never_rejects_a_pipeline_the_builder_accepts() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_debug format=png",
			"from_debug format=pbf",
			"from_debug format=png | filter level_min=2 level_max=5",
			"from_debug format=png | filter bbox=[0,0,10,10] bbox_border=2",
			"from_stacked [ from_debug format=png, from_debug format=png ]",
			"from_debug format=png | raster_format format=webp",
			"from_color color=FF5733",
			"from_color",
		] {
			assert!(
				factory.build_pipeline(parse_vpl(vpl)?).await.is_ok(),
				"precondition: the builder should accept {vpl:?}"
			);
			assert!(
				factory.check(&parse_vpl(vpl)?).is_empty(),
				"check rejected a pipeline the builder accepts: {vpl:?} -> {:?}",
				factory.check(&parse_vpl(vpl)?)
			);
		}
		Ok(())
	}

	/// The converse, for the cases the issue lists: anything `check` rejects,
	/// the builder must reject too — otherwise `--dry-run` would refuse work
	/// that would have succeeded.
	#[tokio::test]
	async fn the_builder_rejects_everything_check_rejects() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_nonsense",
			"from_debug format=png nonsense=1",
			"from_container",
			"from_stacked",
			"filter level_max=5",
			"from_debug format=notaformat",
			"from_debug format=[png, png]",
			"from_debug format=png | filter level_max=abc",
			"from_debug format=png | filter level_max=300",
			"from_debug format=png | raster_flatten color=[0,0]",
			"from_debug format=png | filter bbox=[13.5,52.6,13.3,52.4]",
			"from_debug format=png | filter bbox=[0,0,10]",
			"from_color color=red",
			"from_debug format=png | remap_coords flip_x=yess",
		] {
			assert!(!factory.check(&parse_vpl(vpl)?).is_empty(), "check accepted {vpl:?}");
			assert!(
				factory.build_pipeline(parse_vpl(vpl)?).await.is_err(),
				"the builder accepted something check rejects: {vpl:?}"
			);
		}
		Ok(())
	}

	/// A typo in a flag used to mean the opposite of what it said: anything that
	/// was not `1/true/yes/ok` parsed as `false`, silently. Now it is refused,
	/// by the same function the accessor calls.
	#[test]
	fn a_value_that_is_not_a_boolean_is_reported() {
		assert_eq!(
			problems("from_debug format=png | remap_coords flip_x=yess"),
			["'remap_coords' does not accept 'flip_x=yess': expected a boolean (1/true/yes/ok or 0/false/no), got 'yess'"]
		);
		for spelling in ["1", "true", "YES", " ok ", "0", "false", "no"] {
			let vpl = format!("from_debug format=png | remap_coords flip_x=\"{spelling}\"");
			assert!(problems(&vpl).is_empty(), "{spelling:?} should be a boolean");
		}
	}

	/// The gap #257 was opened for, closed for the field it was opened about.
	/// `color` is a `HexColor`, so the parser that turns `RRGGBB` into bytes is
	/// the one `check` asks — not a second opinion written to agree with it.
	#[test]
	fn a_value_in_the_wrong_format_is_reported() {
		assert_eq!(
			problems("from_color color=red"),
			["'from_color' does not accept 'color=red': Invalid hex color 'red': invalid digit found in string"]
		);
		assert!(problems("from_color color=FF5733").is_empty());
		assert!(problems("from_color color=FF573380").is_empty());
	}

	/// Still open, and the reason `epsg` is left alone: `u32` is the whole of
	/// what the type knows, and "parses as `u32`" is all `check` can say
	/// without a registry to check against.
	#[test]
	fn meaning_the_type_does_not_carry_is_not_checked() {
		assert!(
			problems("from_grid epsg=99999 size=1 bbox=[0,0,1,1]").is_empty(),
			"if this starts failing, `epsg` grew a type that parses and the docs should say so"
		);
	}
}
