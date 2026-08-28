//! The semantic parse entry points.
//!
//! The grammar itself lives in [`cst_parser`](super::cst_parser): there is one parser, and it is
//! the lossless one. Everything here is that parser followed by
//! [`CstFile::lower`](super::CstFile::lower), which drops trivia, decodes escapes, and folds the
//! parameters into the alphabetised map the engine wants.

use anyhow::Result;
use versatiles_derive::context;

use super::{VPLPipeline, VplParseError, parse_cst};

/// Parses VPL text into a [`VPLPipeline`], reporting failures with a machine-readable position.
///
/// Use this over [`parse_vpl`] when the caller wants to *point at* the mistake — to underline it
/// in an editor, or turn it into a language-server range. [`VplParseError`] displays as the same
/// caret-annotated trace `parse_vpl` produces, so nothing is lost by preferring it.
///
/// Callers that want to *edit* the text rather than run it should reach for
/// [`parse_cst`] instead, which keeps the comments and formatting this discards.
///
/// # Examples
///
/// ```
/// use versatiles_pipeline::vpl::parse_vpl_detailed;
///
/// let source = "from_container filename=a | vector_filter zoom";
/// let error = parse_vpl_detailed(source).unwrap_err();
///
/// // The position is data, so it can be sliced straight out of the source. Here the input ran
/// // out, so the span is empty and sits at the end.
/// assert_eq!(error.span, 46..46);
/// assert_eq!(error.message, "expected '=', got end of input");
///
/// // Each context frame says where its construct began, so a caller can widen the span itself.
/// assert_eq!(error.context[0].label, "parsing property");
/// assert_eq!(&source[error.context[0].offset..error.span.end], "zoom");
/// ```
pub fn parse_vpl_detailed(input: &str) -> Result<VPLPipeline, VplParseError> {
	parse_cst(input).map(|file| file.lower())
}

/// Parses VPL text into a [`VPLPipeline`].
///
/// The error displays as `nom`'s context trace in the caret-annotated form the CLI prints. That
/// rendering states the position by drawing it, so a caller that needs the position as data should
/// either use [`parse_vpl_detailed`] or recover the [`VplParseError`] from the returned error with
/// [`anyhow::Error::downcast_ref`].
#[context("Failed to parse VPL input")]
pub fn parse_vpl(input: &str) -> Result<VPLPipeline> {
	parse_vpl_detailed(input).map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;
	use crate::vpl::VPLNode;

	/// Both quote forms must be able to spell the empty string, and an empty value must
	/// survive all the way into the parsed node. See issue #218.
	#[rstest]
	#[case(r#"node a="""#)]
	#[case("node a=''")]
	fn parse_empty_string_value(#[case] input: &str) {
		let expected = VPLNode::from(("node", ("a", "")));
		assert_eq!(parse_vpl(input).unwrap(), VPLPipeline::from(expected));
	}

	/// A malformed escape must be reported against the offending escape character, not against the
	/// opening quote or the backslash.
	#[test]
	fn parse_bad_escape_still_reports_at_the_escape() {
		let error = parse_vpl(r#"node a="foo\qbar""#).unwrap_err();
		let error = error.chain().last().unwrap().to_string();
		let lines = error.trim().lines().collect::<Vec<&str>>();
		assert_eq!(
			&lines[0..4],
			&[
				"0: at line 1:",
				r#"node a="foo\qbar""#,
				"            ^",
				"malformed escape sequence"
			]
		);
	}

	#[test]
	fn test_parse_nodes1() {
		let input = "node1 key1=value1|\nnode2 key2=\"value2\"";
		let expected = VPLPipeline::from(vec![
			VPLNode::from(("node1", ("key1", "value1"))),
			VPLNode::from(("node2", ("key2", "value2"))),
		]);
		assert_eq!(parse_vpl(input).unwrap(), expected);
	}

	#[test]
	fn test_parse_nodes2() {
		let input = "node1 key1=value1|node2 key2=\"value2\"| node3 key3=value3 |\nnode4 key4=value4";
		let expected = VPLPipeline::from(vec![
			VPLNode::from(("node1", ("key1", "value1"))),
			VPLNode::from(("node2", ("key2", "value2"))),
			VPLNode::from(("node3", ("key3", "value3"))),
			VPLNode::from(("node4", ("key4", "value4"))),
		]);
		assert_eq!(parse_vpl(input).unwrap(), expected);
	}

	#[test]
	fn test_parse_nodes3() {
		let input = "node1 key1=value1 [ child1 key2=value2 | child2 key3=\"value3\", child3 key4=value4 ] | node2";
		let expected = VPLPipeline::from(vec![
			VPLNode::from((
				"node1",
				vec![("key1", "value1")],
				vec![
					VPLPipeline::from(vec![
						VPLNode::from(("child1", ("key2", "value2"))),
						VPLNode::from(("child2", ("key3", "value3"))),
					]),
					VPLPipeline::from(VPLNode::from(("child3", ("key4", "value4")))),
				],
			)),
			VPLNode::from("node2"),
		]);
		assert_eq!(parse_vpl(input).unwrap(), expected);
	}

	#[test]
	fn test_parse_nodes4() {
		pub const INPUT: &str = include_str!("../../../testdata/berlin.vpl");

		let expected = VPLPipeline::from(vec![
			VPLNode::from(("from_container", ("filename", "berlin.mbtiles"))),
			VPLNode::from((
				"vector_update_properties",
				vec![
					("data_source_path", "cities.csv"),
					("id_field_data", "city_name"),
					("id_field_tiles", "name"),
					("layer_name", "place_labels"),
				],
			)),
		]);
		assert_eq!(parse_vpl(INPUT).unwrap(), expected);
	}

	#[rstest]
	#[case("node [ child key=value ] node", &[
		"0: at line 1:",
		"node [ child key=value ] node",
		"                         ^",
		"unexpected input"
	])]
	#[case("node child key=value ]", &[
		"0: at line 1:",
		"node child key=value ]",
		"           ^",
		"expected '=', found k",
		"",
		"1: at line 1, in parsing property:",
		"node child key=value ]",
		"     ^",
		"",
		"2: at line 1, in parsing node:",
		"node child key=value ]",
		"^",
		"",
		"3: at line 1, in parsing pipeline:",
		"node child key=value ]",
		"^"
	])]
	#[case("node key=\"2.1", &[
		"0: at line 1:",
		"node key=\"2.1",
		"             ^",
		"expected '\"', got end of input",
		"",
		"1: at line 1, in parsing double quoted string:",
		"node key=\"2.1",
		"         ^",
		"",
		"2: at line 1, in parsing property:",
		"node key=\"2.1",
		"     ^",
		"",
		"3: at line 1, in parsing node:",
		"node key=\"2.1",
		"^",
		"",
		"4: at line 1, in parsing pipeline:",
		"node key=\"2.1",
		"^"
	])]
	#[case("node [n key=2,1]", &[
		"0: at line 1:",
		"node [n key=2,1]",
		"             ^",
		"expected ']', found ,",
		"",
		"1: at line 1, in parsing sources:",
		"node [n key=2,1]",
		"     ^",
		"",
		"2: at line 1, in parsing node:",
		"node [n key=2,1]",
		"^",
		"",
		"3: at line 1, in parsing pipeline:",
		"node [n key=2,1]",
		"^"
	])]
	#[case("node [n key=2]]", &[
		"0: at line 1:",
		"node [n key=2]]",
		"              ^",
		"unexpected input"
	])]
	#[case("node [ ] [ ]", &[
		"0: at line 1:",
		"node [ ] [ ]",
		"         ^",
		"unexpected input"
	])]
	#[case("node [ a; b ]", &[
		"0: at line 1:",
		"node [ a; b ]",
		"        ^",
		"expected ']', found ;",
		"",
		"1: at line 1, in parsing sources:",
		"node [ a; b ]",
		"     ^",
		"",
		"2: at line 1, in parsing node:",
		"node [ a; b ]",
		"^",
		"",
		"3: at line 1, in parsing pipeline:",
		"node [ a; b ]",
		"^"
	])]
	#[case("node | | node", &[
		"0: at line 1:",
		"node | | node",
		"       ^",
		"unexpected character",
		"",
		"1: at line 1, in parsing name:",
		"node | | node",
		"       ^",
		"",
		"2: at line 1, in parsing node identifier:",
		"node | | node",
		"       ^",
		"",
		"3: at line 1, in parsing node:",
		"node | | node",
		"       ^",
		"",
		"4: at line 1, in parsing pipeline:",
		"node | | node",
		"^"
	])]
	fn test_error_messages(#[case] vpl: &str, #[case] message: &[&str]) {
		let error = parse_vpl(vpl).unwrap_err().chain().last().unwrap().to_string();
		let lines = error.trim().lines().collect::<Vec<&str>>();
		assert_eq!(lines, message, "for vpl: '{vpl}'");
	}
}
