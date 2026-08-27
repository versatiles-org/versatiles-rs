//! VPL text handling for JavaScript: parsing pipelines and writing them back out.
//!
//! Both directions go through `versatiles_pipeline`, so the quoting and grammar rules exist in
//! exactly one place. The TypeScript `VPL` builder used to carry its own serialiser, which drifted
//! from the grammar and emitted pipelines that would not parse — sources before parameters, and
//! array items that were never quoted.
//!
//! Pipelines cross the boundary as JSON strings, matching `TileSource.fromPipeline`.

use std::collections::BTreeMap;

use anyhow::Context;
use napi::Result;
use napi_derive::napi;
use serde::Deserialize;
use serde_json::{Value, json};
use versatiles::pipeline::vpl::{CstFile, VPLNode, VPLPipeline, parse_cst, parse_vpl_detailed};

use crate::macros::NapiResultExt;

/// One operation in the JSON pipeline representation the TypeScript builder produces.
#[derive(Deserialize)]
pub(crate) struct PipelineStep {
	pub(crate) name: String,
	pub(crate) params: std::collections::HashMap<String, Value>,
	#[serde(default)]
	pub(crate) sources: Vec<Vec<PipelineStep>>,
}

pub(crate) fn steps_to_pipeline(steps: Vec<PipelineStep>) -> VPLPipeline {
	VPLPipeline::new(steps.into_iter().map(step_to_node).collect())
}

fn value_to_strings(value: Value) -> Vec<String> {
	fn scalar(value: Value) -> String {
		match value {
			Value::String(s) => s,
			Value::Number(n) => n.to_string(),
			Value::Bool(b) => b.to_string(),
			other => other.to_string(),
		}
	}

	match value {
		Value::Array(items) => items.into_iter().map(scalar).collect(),
		other => vec![scalar(other)],
	}
}

fn step_to_node(step: PipelineStep) -> VPLNode {
	let properties: BTreeMap<String, Vec<String>> = step
		.params
		.into_iter()
		.map(|(key, value)| (key, value_to_strings(value)))
		.collect();

	VPLNode {
		name: step.name,
		properties,
		sources: step.sources.into_iter().map(steps_to_pipeline).collect(),
	}
}

/// The inverse of [`steps_to_pipeline`], for handing a parsed pipeline back to JavaScript.
///
/// A parameter with exactly one value becomes a string and anything else becomes an array, which
/// is the shape the builder produces and `steps_to_pipeline` accepts — so a pipeline can be parsed,
/// handed to JavaScript, and sent straight back.
fn pipeline_to_steps(pipeline: &VPLPipeline) -> Value {
	let steps: Vec<Value> = pipeline
		.pipeline
		.iter()
		.map(|node| {
			let params: serde_json::Map<String, Value> = node
				.properties
				.iter()
				.map(|(key, values)| {
					let value = match values.as_slice() {
						[single] => Value::String(single.clone()),
						many => Value::Array(many.iter().cloned().map(Value::from).collect()),
					};
					(key.clone(), value)
				})
				.collect();

			let mut step = json!({ "name": node.name, "params": params });
			if !node.sources.is_empty() {
				let sources: Vec<Value> = node.sources.iter().map(pipeline_to_steps).collect();
				step["sources"] = Value::Array(sources);
			}
			step
		})
		.collect();

	Value::Array(steps)
}

/// Writes a pipeline as VPL text.
///
/// Takes the JSON produced by `VPL.toJSON()` and returns text that `TileSource.fromVpl` accepts.
/// Quoting is decided by the grammar, so values containing spaces, quotes or newlines are handled
/// without the caller knowing the rules.
///
/// @param stepsJson - JSON string describing the pipeline steps
/// @returns the pipeline as VPL text
// `#[expect]` cannot be used on these: `#[napi]` re-emits the function, so the
// suppression works by span but the expectation is never marked fulfilled.
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi generates by-value `String` arguments for JavaScript strings"
)]
#[napi]
pub fn stringify_vpl(steps_json: String) -> Result<String> {
	let steps: Vec<PipelineStep> = serde_json::from_str(&steps_json)
		.context("Invalid pipeline JSON")
		.to_napi()?;
	Ok(steps_to_pipeline(steps).to_string())
}

/// Renders a parse failure as the JSON both entry points return.
fn error_to_json(error: &versatiles::pipeline::vpl::VplParseError) -> Value {
	json!({
		"span": { "start": error.span.start, "end": error.span.end },
		"message": error.message,
		"context": error
			.context
			.iter()
			.map(|frame| json!({ "label": frame.label, "offset": frame.offset }))
			.collect::<Vec<Value>>(),
		"trace": error.to_string(),
	})
}

/// Parses VPL text into the JSON pipeline representation.
///
/// Returns a JSON string rather than throwing, because an editor parses on every keystroke and a
/// syntax error there is an expected state rather than an exception:
///
/// - on success, `{"ok": true, "pipeline": [...]}` — the same shape `VPL.toJSON()` produces
/// - on failure, `{"ok": false, "error": {"span": {...}, "message": "...", "context": [...],
///   "trace": "..."}}`
///
/// `span` holds **byte** offsets into the input, so a caller can convert to whatever unit it
/// counts in. `trace` is the caret-annotated rendering the CLI prints.
///
/// @param vpl - the VPL text to parse
/// @returns a JSON string describing either the pipeline or the error
#[napi]
#[must_use]
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi generates by-value `String` arguments for JavaScript strings"
)]
pub fn parse_vpl(vpl: String) -> String {
	let result = match parse_vpl_detailed(&vpl) {
		Ok(pipeline) => json!({ "ok": true, "pipeline": pipeline_to_steps(&pipeline) }),
		Err(error) => json!({ "ok": false, "error": error_to_json(&error) }),
	};
	result.to_string()
}

/// Checks VPL text against the known operations, without building the pipeline.
///
/// Building opens files and fetches URLs, so it cannot run on a keystroke — and it conflates two
/// failures. `from_container filename=missing.mbtiles` is the *world* being wrong; `from_debug
/// format=png nonsense=1` is the *pipeline* being wrong. This reports only the second kind, and
/// performs no I/O.
///
/// Returns a JSON string, with the same result shape as [`parse_vpl`]:
///
/// - on success, `{"ok": true, "problems": [{"message": "...", "path": [0], "property": "nonsense"}]}`
///   — an empty `problems` array means nothing is wrong with the pipeline
/// - on failure, `{"ok": false, "error": {...}}` — the text did not parse at all
///
/// `path` addresses the offending node by index: `[2]` is the third node of the pipeline, and
/// descending into a node's bracketed sources appends the source index and then the node index.
/// The same walk applies to the tree from `parseVplCst`, which is how this becomes a span.
///
/// A parameter whose type has a parser is handed to it, so `format=notaformat` is reported and
/// the alias `format=pbf` is not — the parser decides, never the list of values a picker offers.
/// A value whose type has no parser is deliberately *not* validated, because doing so would
/// reject VPL that runs: `color=red` passes here and fails at build time.
///
/// @param vpl - the VPL text to check
/// @returns a JSON string describing either the problems found or a parse error
#[napi]
#[must_use]
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi generates by-value `String` arguments for JavaScript strings"
)]
pub fn check_vpl(vpl: String) -> String {
	let result = match parse_vpl_detailed(&vpl) {
		Ok(pipeline) => {
			let problems = versatiles::pipeline::check_pipeline(&pipeline)
				.into_iter()
				.map(|problem| {
					json!({
						"message": problem.message,
						"path": problem.path,
						"property": problem.property,
					})
				})
				.collect::<Vec<_>>();
			json!({ "ok": true, "problems": problems })
		}
		Err(error) => json!({ "ok": false, "error": error_to_json(&error) }),
	};
	result.to_string()
}

/// Parses VPL text into the lossless syntax tree.
///
/// Unlike [`parse_vpl`], which returns the semantic pipeline the engine runs, this keeps
/// everything the file contains — comments, whitespace, the order the parameters were written in,
/// and which quotes the author chose. Hold this tree if you mean to edit a file somebody wrote:
/// [`stringify_vpl_cst`] prints it back byte for byte, so the parts nobody touched stay untouched.
///
/// Returns a JSON string, with the same result shape as [`parse_vpl`]:
///
/// - on success, `{"ok": true, "cst": {...}}`
/// - on failure, `{"ok": false, "error": {...}}`
///
/// Every token carries a `span` of **byte** offsets into the input.
///
/// @param vpl - the VPL text to parse
/// @returns a JSON string describing either the syntax tree or the error
#[napi]
#[must_use]
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi generates by-value `String` arguments for JavaScript strings"
)]
pub fn parse_vpl_cst(vpl: String) -> String {
	match parse_cst(&vpl) {
		Ok(file) => json!({ "ok": true, "cst": file }),
		Err(error) => json!({ "ok": false, "error": error_to_json(&error) }),
	}
	.to_string()
}

/// Writes a lossless syntax tree back out as VPL text.
///
/// The inverse of [`parse_vpl_cst`]: an unedited tree prints back to exactly the text it was
/// parsed from. Fields that describe formatting — `leading`, `trailing`, `span` — may be omitted
/// when building a tree by hand, so a minimal node is `{"name": {"text": "from_debug"}}`.
///
/// Note that `span` values are stale after an edit; they describe where a token *was*. Parse the
/// printed text again to get fresh ones.
///
/// @param cstJson - JSON string describing the syntax tree
/// @returns the tree as VPL text
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi generates by-value `String` arguments for JavaScript strings"
)]
#[napi]
pub fn stringify_vpl_cst(cst_json: String) -> Result<String> {
	let file: CstFile = serde_json::from_str(&cst_json)
		.context("Invalid VPL syntax tree JSON")
		.to_napi()?;
	Ok(file.to_string())
}

/// Reformats a lossless syntax tree, keeping the comments in it.
///
/// [`stringify_vpl`] formats the semantic pipeline, which has already forgotten the comments, so
/// "reformat this file" and "keep what I wrote" are otherwise exclusive. This applies the same
/// layout — one tab per level, `|` at the pipeline's own level, parameters one level in — to the
/// tree that still has them.
///
/// Whitespace and nothing else is rewritten: parameters keep the order they were written in, and
/// values keep the quotes the author chose. Every comment keeps the token it was attached to and
/// takes that token's line and indentation.
///
/// Returns the formatted tree rather than the text, so the `span` of every token is fresh and
/// addresses the formatted result. Print it with [`stringify_vpl_cst`].
///
/// @param cstJson - JSON string describing the syntax tree
/// @returns a JSON string describing the formatted syntax tree
#[allow(
	clippy::needless_pass_by_value,
	reason = "napi generates by-value `String` arguments for JavaScript strings"
)]
#[napi]
pub fn format_vpl_cst(cst_json: String) -> Result<String> {
	let mut file: CstFile = serde_json::from_str(&cst_json)
		.context("Invalid VPL syntax tree JSON")
		.to_napi()?;
	file.format();
	serde_json::to_string(&file)
		.context("Failed to serialize the formatted VPL syntax tree")
		.to_napi()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parsed(vpl: &str) -> Value {
		serde_json::from_str(&parse_vpl(vpl.to_string())).unwrap()
	}

	#[test]
	fn stringify_quotes_what_the_grammar_requires() {
		let steps = r#"[{"name":"from_container","params":{"filename":"a b.versatiles"}}]"#;
		assert_eq!(
			stringify_vpl(steps.to_string()).unwrap(),
			"from_container filename='a b.versatiles'"
		);
	}

	/// The builder used to write sources *before* the parameters, which the grammar rejects.
	#[test]
	fn stringify_puts_sources_after_parameters() {
		let steps = r#"[{"name":"from_stacked_raster","params":{"level_min":5},
			"sources":[[{"name":"a","params":{}}],[{"name":"b","params":{}}]]}]"#;
		let vpl = stringify_vpl(steps.to_string()).unwrap();

		assert_eq!(vpl, "from_stacked_raster level_min=5 [ a, b ]");
		assert!(parsed(&vpl)["ok"].as_bool().unwrap(), "{vpl} should parse");
	}

	/// The builder used to join array items without quoting them, so any item containing a space
	/// produced text that would not parse.
	#[test]
	fn stringify_quotes_array_items() {
		let steps = r#"[{"name":"vector_filter_layers","params":{"layer":["place","water park"]}}]"#;
		let vpl = stringify_vpl(steps.to_string()).unwrap();

		assert_eq!(vpl, "vector_filter_layers layer=[place, 'water park']");
		assert!(parsed(&vpl)["ok"].as_bool().unwrap(), "{vpl} should parse");
	}

	#[test]
	fn stringify_rejects_malformed_json() {
		assert!(stringify_vpl("not json".to_string()).is_err());
	}

	#[test]
	fn parse_returns_the_builder_shape() {
		let result = parsed("from_container filename='a b' | filter level_min=5");
		assert!(result["ok"].as_bool().unwrap());

		let pipeline = &result["pipeline"];
		assert_eq!(pipeline[0]["name"], "from_container");
		assert_eq!(pipeline[0]["params"]["filename"], "a b");
		assert_eq!(pipeline[1]["params"]["level_min"], "5");
	}

	/// Parsing and stringifying have to be inverse, or JavaScript cannot edit a pipeline it read.
	#[test]
	fn parse_and_stringify_round_trip() {
		for vpl in [
			"from_container filename='a b'",
			"vector_filter_layers layer=[place, water]",
			"from_stacked [ a key=1, b ] | filter level_min=5",
			"node key=''",
		] {
			let result = parsed(vpl);
			assert!(result["ok"].as_bool().unwrap(), "{vpl} should parse");
			let back = stringify_vpl(result["pipeline"].to_string()).unwrap();
			assert_eq!(back, vpl, "round trip differed");
		}
	}

	#[test]
	fn parse_reports_errors_as_data() {
		let result = parsed("from_container filename=a | vector_filter zoom");
		assert!(!result["ok"].as_bool().unwrap());

		let error = &result["error"];
		assert_eq!(error["span"]["start"], 46);
		assert_eq!(error["message"], "expected '=', got end of input");
		assert_eq!(error["context"][0]["label"], "parsing property");
		assert_eq!(error["context"][0]["offset"], 42);
		assert!(error["trace"].as_str().unwrap().contains("at line 1"));
	}

	/// Byte offsets, so a caller can decide how it wants to count.
	#[test]
	fn parse_reports_byte_offsets() {
		let result = parsed("node a=\"Grüße\" b=!");
		assert_eq!(result["error"]["span"]["start"], 19);
		assert_eq!(result["error"]["span"]["end"], 20);
	}

	/// Holding the tree elsewhere is only safe if it comes back unchanged.
	#[test]
	fn cst_round_trips_through_json() {
		for vpl in [
			"# a comment\nfrom_container  filename = 'berlin.versatiles'  # note\n",
			"from_stacked [ a key=1 | b, c ] | filter level_min=5",
			"node key=[\"\", x, '']",
		] {
			let parsed: Value = serde_json::from_str(&parse_vpl_cst(vpl.to_string())).unwrap();
			assert!(parsed["ok"].as_bool().unwrap(), "{vpl} should parse");

			let printed = stringify_vpl_cst(parsed["cst"].to_string()).unwrap();
			assert_eq!(printed, vpl, "comments or formatting were lost");
		}
	}

	/// The Format command an editor offers: layout is rewritten, the comments
	/// and the author's quoting are not, and the returned tree can be printed.
	#[test]
	fn formatting_keeps_the_comments_and_returns_a_printable_tree() {
		let source = "# which file\nfrom_container   filename = 'berlin.versatiles' | filter level_min=5";
		let parsed: Value = serde_json::from_str(&parse_vpl_cst(source.to_string())).unwrap();

		let formatted = format_vpl_cst(parsed["cst"].to_string()).unwrap();
		let printed = stringify_vpl_cst(formatted).unwrap();

		assert_eq!(
			printed,
			"# which file\nfrom_container\n\tfilename='berlin.versatiles'\n| filter\n\tlevel_min=5\n"
		);
	}

	#[test]
	fn formatting_rejects_a_tree_that_is_not_one() {
		assert!(format_vpl_cst("{\"nonsense\": true}".to_string()).is_err());
	}

	/// Editing the tree changes only what was edited.
	#[test]
	fn editing_the_tree_leaves_the_rest_alone() {
		let source = "# which file\nfrom_container  filename = 'berlin.versatiles'  # note";
		let mut parsed: Value = serde_json::from_str(&parse_vpl_cst(source.to_string())).unwrap();

		let value = &mut parsed["cst"]["pipeline"]["nodes"][0]["value"]["properties"][0]["value"];
		value["token"]["text"] = Value::String("hamburg.versatiles".to_string());
		value["quote"] = Value::String("bare".to_string());

		let printed = stringify_vpl_cst(parsed["cst"].to_string()).unwrap();
		assert_eq!(
			printed,
			"# which file\nfrom_container  filename = hamburg.versatiles  # note"
		);
	}

	#[test]
	fn cst_reports_errors_the_same_way_as_the_pipeline_parser() {
		let parsed: Value = serde_json::from_str(&parse_vpl_cst("node ][".to_string())).unwrap();
		assert!(!parsed["ok"].as_bool().unwrap());
		assert!(!parsed["error"]["message"].as_str().unwrap().is_empty());
		assert!(parsed["error"]["trace"].as_str().unwrap().contains("at line 1"));
	}

	#[test]
	fn stringify_cst_rejects_malformed_json() {
		assert!(stringify_vpl_cst("not json".to_string()).is_err());
	}
}
