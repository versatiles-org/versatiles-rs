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
	/// so an editor can underline all of them at once. An empty result means
	/// nothing is wrong *with the pipeline* — building it can still fail because
	/// a file is missing, or because a value has the right name but the wrong
	/// format (`color=red` is not hex).
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
	for key in node.properties.keys() {
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

		// Values are deliberately *not* checked, not even for enum-typed
		// parameters. `VPLFieldMeta::enum_variants` carries the canonical names
		// used to render documentation and TypeScript unions, and excludes the
		// aliases the parsers accept — `TileFormat` takes "pbf" and "jpeg"
		// while listing only "mvt" and "jpg" (see `TileFormat::variants`).
		// Comparing against it would reject `from_debug format=pbf`, which
		// builds perfectly well. Rejecting valid VPL is worse than missing an
		// invalid value, so this waits until the metadata carries the accepted
		// set rather than the printable one.
		let _ = field;
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

	/// Recorded on purpose, like `value_formats_are_not_checked`: an invalid
	/// enum value is not reported, because `enum_variants` lists canonical names
	/// only and the parsers accept aliases besides. Checking against it would
	/// reject `from_debug format=pbf`, which builds.
	#[test]
	fn enum_values_are_not_checked() {
		assert!(problems("from_debug format=nonsense").is_empty());
		assert!(
			problems("from_debug format=pbf").is_empty(),
			"an accepted alias must never be reported"
		);
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
		] {
			assert!(!factory.check(&parse_vpl(vpl)?).is_empty(), "check accepted {vpl:?}");
			assert!(
				factory.build_pipeline(parse_vpl(vpl)?).await.is_err(),
				"the builder accepted something check rejects: {vpl:?}"
			);
		}
		Ok(())
	}

	/// Recorded on purpose: a value with the right name but the wrong *format*
	/// is invisible to a metadata-driven check. Closing this gap needs a format
	/// hint on VPLFieldMeta (the second half of #224).
	#[test]
	fn value_formats_are_not_checked() {
		assert!(
			problems("from_color color=red").is_empty(),
			"if this starts failing, VPLFieldMeta grew format hints and the docs should say so"
		);
	}
}
