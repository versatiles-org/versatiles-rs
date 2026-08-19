//! VPL serialisation.
//!
//! The counterpart to [`parser`](super::parser): where the parser turns VPL text into a
//! [`VPLPipeline`], the implementations here turn one back into text that the parser accepts.
//! That makes it possible to write a `.vpl` file from a pipeline assembled in code, and to
//! round-trip the parser against itself in tests.
//!
//! Two styles, for two audiences:
//!
//! - [`Display`] never breaks a line. It is what a log message, an error, or a test assertion
//!   wants, and what the Node bindings emit.
//! - [`VPLPipeline::to_string_pretty`] puts one operation per line. It is what a `.vpl` file a
//!   person reads wants, since a pipeline of any length is unreadable on one line.
//!
//! Both normalise the same way — the list below applies to either.
//!
//! Values are written with the least punctuation that parses back: bare where the grammar
//! allows it, single quotes next (they need no escapes), double quotes with escapes last.
//! Every string has a representation, including the empty one, so serialisation cannot fail.
//!
//! # Round-tripping
//!
//! `parse_vpl(&pipeline.to_string())` reproduces `pipeline` — but the printed text is not
//! necessarily the text it was parsed from, because the parser discards information it does
//! not need:
//!
//! - **Comments** are folded away by the whitespace parser and cannot be recovered.
//! - **Parameter order** is lost: `properties` is a [`BTreeMap`](std::collections::BTreeMap),
//!   so parameters come back alphabetised.
//! - **Formatting** (line breaks, indentation, which quote form the author chose, whether a
//!   single value was written as `a=x` or `a=[x]`) is normalised.
//!
//! So this is a serialiser, not a formatter: it is the right tool for emitting a pipeline
//! built programmatically, and the wrong one for rewriting a file a user is editing.
//!
//! # Preconditions
//!
//! [`Display`] cannot fail, so it assumes what the grammar requires of the parts that have no
//! quoted form: node names and parameter keys must be valid identifiers (an ASCII letter
//! followed by ASCII alphanumerics, `_` or `-`). That holds for anything the parser produced.
//! A node built by hand with, say, an empty name or a key containing a space will print text
//! that does not parse back. Likewise an empty [`VPLPipeline`] prints as the empty string,
//! which is not a valid pipeline — the grammar requires at least one node.

use std::{
	borrow::Cow,
	fmt::{self, Display, Write},
};

use super::{VPLNode, VPLPipeline};

/// Returns `true` if `value` can be written without quotes.
///
/// Mirrors `parse_unquoted_value`: a non-empty run of ASCII alphanumerics and `.`, `-`, `_`.
/// Note that this is ASCII-only — `Grüße` needs quoting even though it looks word-like.
fn is_bare(value: &str) -> bool {
	!value.is_empty()
		&& value
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Renders one value using the least punctuation that parses back.
///
/// Bare if the grammar allows it; otherwise single-quoted, which needs no escapes and so
/// keeps backslashes readable (`'C:\dir'`); otherwise double-quoted with `\\`, `\"`, `\n`
/// and `\t` escaped. Single quotes are skipped for values containing `'` (they have no escape
/// mechanism) or control characters (double quotes can spell the common ones).
///
/// Shared with [`cst`](super::cst) so that the quoting rules exist in exactly one place.
pub(super) fn quote(value: &str) -> Cow<'_, str> {
	if is_bare(value) {
		return Cow::Borrowed(value);
	}

	if !value.contains('\'') && !value.chars().any(char::is_control) {
		return Cow::Owned(format!("'{value}'"));
	}

	let mut result = String::with_capacity(value.len() + 2);
	result.push('"');
	for c in value.chars() {
		match c {
			'\\' => result.push_str("\\\\"),
			'"' => result.push_str("\\\""),
			'\n' => result.push_str("\\n"),
			'\t' => result.push_str("\\t"),
			// Any other character, control ones included, is literal inside double quotes.
			_ => result.push(c),
		}
	}
	result.push('"');
	Cow::Owned(result)
}

/// Renders one parameter as `key=value`.
///
/// A single value is written plainly; anything else needs the bracket form, which includes the
/// empty list, since `a=` alone is not a value.
fn property(key: &str, values: &[String]) -> String {
	let mut out = format!("{key}=");
	if let [value] = values {
		out.push_str(&quote(value));
	} else {
		out.push('[');
		for (i, value) in values.iter().enumerate() {
			if i > 0 {
				out.push_str(", ");
			}
			out.push_str(&quote(value));
		}
		out.push(']');
	}
	out
}

/// One tab per nesting level.
///
/// Tabs rather than spaces so that indentation stays *identifiable*: a tab reaches the output
/// only as structure, since a tab inside a value is escaped to `\t` and a bare value cannot
/// contain one. The same holds for the `\n` between lines. That makes both replaceable exactly
/// — see [`VPLPipeline::to_string_pretty`] — which is a configuration mechanism that costs no
/// API at all.
fn indent(level: usize) -> String {
	"\t".repeat(level)
}

impl VPLPipeline {
	/// Writes the pipeline across several lines, one operation per line.
	///
	/// The counterpart to [`Display`], which never breaks a line: this is for `.vpl` files
	/// people read, where a long pipeline on one line is unreadable. It normalises exactly as
	/// much as `Display` does — comments and the author's parameter order are still gone — so
	/// it is a serialiser too, not a formatter. Rewriting a file someone is editing needs the
	/// CST, which keeps comments and source order.
	///
	/// Every operation goes on its own line and every parameter on its own, always — a layout
	/// that never depends on how long a value happens to be is one where adding a parameter
	/// changes exactly one line. The `|` starts the line rather than ending the previous one and
	/// sits at the pipeline's own level, so the operations read as a single column and appending
	/// a stage is a one-line diff too. Parameters and nested sources indent one level in.
	///
	/// # Indentation and line endings
	///
	/// One tab per level, `\n` between lines. Both are *only* ever structure: a tab or newline
	/// inside a value is escaped (`\t`, `\n`), and a bare value cannot contain either. So a
	/// caller who wants something else can rewrite it exactly, with no ambiguity to get wrong:
	///
	/// ```
	/// # use versatiles_pipeline::vpl::parse_vpl;
	/// # let pipeline = parse_vpl("from_debug format=png").unwrap();
	/// let two_spaces_crlf = pipeline.to_string_pretty().replace('\t', "  ").replace('\n', "\r\n");
	/// ```
	///
	/// The parser accepts either form back, so a rewritten file still round-trips.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_pipeline::vpl::parse_vpl;
	///
	/// let pipeline = parse_vpl("from_debug format=png | filter level_min=2").unwrap();
	/// assert_eq!(pipeline.to_string_pretty(), "from_debug\n\tformat=png\n| filter\n\tlevel_min=2");
	///
	/// // ...and it parses back to the same pipeline.
	/// assert_eq!(parse_vpl(&pipeline.to_string_pretty()).unwrap(), pipeline);
	/// ```
	#[must_use]
	pub fn to_string_pretty(&self) -> String {
		pretty_pipeline(self, 0)
	}
}

/// A pipeline whose operations sit at `level`.
fn pretty_pipeline(pipeline: &VPLPipeline, level: usize) -> String {
	let mut out = String::new();
	for (i, node) in pipeline.pipeline.iter().enumerate() {
		if i > 0 {
			// The `|` leads its line and stays at the pipeline's own level, so the operations
			// line up in one column however deeply their parameters nest.
			write!(out, "\n{}| ", indent(level)).expect("writing to a String");
		}
		out.push_str(&pretty_node(node, level));
	}
	out
}

/// A node whose name sits at `level`, with its parameters and sources one level in.
fn pretty_node(node: &VPLNode, level: usize) -> String {
	let mut out = node.name.clone();

	for (key, values) in &node.properties {
		write!(out, "\n{}{}", indent(level + 1), property(key, values)).expect("writing to a String");
	}

	if !node.sources.is_empty() {
		out.push_str(" [");
		for (i, source) in node.sources.iter().enumerate() {
			if i > 0 {
				// The comma belongs to the source it follows, on the line that source ends on.
				out.push(',');
			}
			write!(out, "\n{}{}", indent(level + 1), pretty_pipeline(source, level + 1)).expect("writing to a String");
		}
		// The grammar has no trailing comma, so the closing bracket gets its own line at the
		// node's level.
		write!(out, "\n{}]", indent(level)).expect("writing to a String");
	}

	out
}

impl Display for VPLNode {
	/// Writes the node as VPL text, using the least punctuation that parses back.
	///
	/// The same normalisation as [`VPLPipeline`]'s: parameters are alphabetised, quoting is
	/// reduced to whatever the grammar allows, and a one-element list collapses to a plain value.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_pipeline::vpl::VPLNode;
	///
	/// let node = VPLNode::try_from_str("filter  level_min=5 layer=[\"place\"]").unwrap();
	/// assert_eq!(node.to_string(), "filter layer=place level_min=5");
	/// ```
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.name)?;

		for (key, values) in &self.properties {
			write!(f, " {}", property(key, values))?;
		}

		if !self.sources.is_empty() {
			f.write_str(" [ ")?;
			for (i, source) in self.sources.iter().enumerate() {
				if i > 0 {
					f.write_str(", ")?;
				}
				write!(f, "{source}")?;
			}
			f.write_str(" ]")?;
		}

		Ok(())
	}
}

impl Display for VPLPipeline {
	/// Writes the pipeline as VPL text, using the least punctuation that parses back.
	///
	/// The output re-parses to an equal pipeline, but it is **not** necessarily the text it was
	/// parsed from — the parser discards what it does not need, so a load-edit-save cycle is lossy:
	///
	/// - comments are gone; the whitespace parser folds them away
	/// - parameters come back alphabetised, because they are held in a `BTreeMap`
	/// - line breaks, indentation, the author's choice of quotes, and `a=[x]` versus `a=x` are all
	///   normalised
	///
	/// So this is the right tool for writing out a pipeline built in code, and the wrong one for
	/// rewriting a file someone is editing.
	///
	/// Values are quoted only as far as the grammar requires: bare where it allows, single quotes
	/// next (no escapes needed, so backslashes stay readable), double quotes with escapes last.
	/// Every value has a representation, so this cannot fail — with one exception: an empty
	/// pipeline writes as the empty string, which is not valid VPL, since the grammar requires at
	/// least one node. Node names and parameter keys have no quoted form either, so a node built by
	/// hand with a key containing a space will write text that does not parse back.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_pipeline::vpl::parse_vpl;
	///
	/// // quoting is reduced and the comment is dropped
	/// let pipeline = parse_vpl("from_container  filename='berlin.versatiles' # a comment").unwrap();
	/// assert_eq!(pipeline.to_string(), "from_container filename=berlin.versatiles");
	///
	/// // parameters come back in alphabetical order, not source order
	/// let pipeline = parse_vpl("node zebra=1 alpha=2").unwrap();
	/// assert_eq!(pipeline.to_string(), "node alpha=2 zebra=1");
	///
	/// // but the round trip preserves meaning
	/// assert_eq!(parse_vpl(&pipeline.to_string()).unwrap(), pipeline);
	/// ```
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for (i, node) in self.pipeline.iter().enumerate() {
			if i > 0 {
				f.write_str(" | ")?;
			}
			write!(f, "{node}")?;
		}
		Ok(())
	}
}

#[cfg(test)]
mod pretty_tests {
	use crate::vpl::parse_vpl;

	fn pretty(text: &str) -> String {
		parse_vpl(text).expect("valid VPL").to_string_pretty()
	}

	#[test]
	fn every_operation_and_every_parameter_gets_its_own_line() {
		assert_eq!(
			pretty("from_debug format=png | filter level_min=2 level_max=9"),
			"from_debug\n\tformat=png\n| filter\n\tlevel_max=9\n\tlevel_min=2"
		);
	}

	#[test]
	fn a_node_without_parameters_stays_on_one_line() {
		assert_eq!(pretty("from_debug | filter"), "from_debug\n| filter");
	}

	#[test]
	fn sources_nest_one_level_at_a_time() {
		assert_eq!(
			pretty(
				"from_stacked [ from_container filename=a.versatiles | filter level_min=5, from_container filename=b.versatiles ] | filter level_max=9"
			),
			concat!(
				"from_stacked [\n",
				"\tfrom_container\n",
				"\t\tfilename=a.versatiles\n",
				"\t| filter\n",
				"\t\tlevel_min=5,\n",
				"\tfrom_container\n",
				"\t\tfilename=b.versatiles\n",
				"]\n",
				"| filter\n",
				"\tlevel_max=9"
			)
		);
	}

	#[test]
	fn the_closing_bracket_carries_no_comma() {
		// The grammar rejects a trailing comma, so the last source cannot have one.
		let pretty = pretty("from_stacked [ from_debug format=png, from_debug format=jpg ]");
		assert!(!pretty.contains(",\n]"), "{pretty}");
		assert_eq!(pretty.matches(',').count(), 1, "{pretty}");
	}

	/// Indentation and line endings are configured by rewriting them, which only works while a
	/// tab or newline cannot reach the output any other way.
	#[test]
	fn tabs_and_newlines_are_structure_and_nothing_else() {
		// Values holding both, from the parser and built by hand, in every quoting form.
		let cases = [
			"node a='tab\there' b=\"nl\\nthere\" c='has space'",
			"node a=\"tab\\there\" b=[\"x\\ty\", \"p\\nq\"]",
		];

		for case in cases {
			let pipeline = parse_vpl(case).expect("valid VPL");
			let text = pipeline.to_string_pretty();
			for line in text.split('\n') {
				let body = line.trim_start_matches('\t');
				assert!(!body.contains('\t'), "a tab escaped the indentation: {text:?}");
			}
			// And a caller's rewrite of both still parses back to the same pipeline.
			let rewritten = text.replace('\t', "  ").replace('\n', "\r\n");
			assert_eq!(
				parse_vpl(&rewritten).expect(&rewritten),
				pipeline,
				"rewritten:\n{rewritten}"
			);
		}
	}

	/// Whatever it prints has to parse back to what it was printed from — the property that
	/// makes this a serialiser rather than a pretty-printer.
	#[test]
	fn everything_it_prints_parses_back() {
		let cases = [
			"from_debug format=png",
			"from_debug format=png | filter level_min=2",
			"filter layer=[\"a\", \"b\"] level_min=2",
			"filter layer=[]",
			"node key='a b' other=\"tab\\there\"",
			"from_stacked [ from_debug format=png, from_debug format=jpg | filter level_min=1 ]",
			"from_stacked [ from_stacked [ from_debug format=png ] | filter level_min=3 ] | filter level_max=8",
		];

		for case in cases {
			let pipeline = parse_vpl(case).expect("valid VPL");
			let printed = pipeline.to_string_pretty();
			assert_eq!(parse_vpl(&printed).expect(&printed), pipeline, "printed as:\n{printed}");
		}
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;
	use crate::vpl::parse_vpl;

	#[rstest]
	// bare, where the grammar allows it
	#[case("value", "value")]
	#[case("a.b-c_d", "a.b-c_d")]
	#[case("-2.0", "-2.0")]
	// single quotes for anything else that has no `'` and no control character
	#[case("", "''")]
	#[case("foo bar", "'foo bar'")]
	#[case("Grüße", "'Grüße'")] // non-ASCII is not bare
	#[case("a=b|c", "'a=b|c'")]
	#[case(r"C:\dir", r"'C:\dir'")] // single quotes keep backslashes readable
	#[case("say \"hi\"", "'say \"hi\"'")]
	// double quotes as the fallback, for `'` and for control characters
	#[case("it's", r#""it's""#)]
	#[case("it's a \"quote\"", r#""it's a \"quote\"""#)]
	#[case("line\nbreak", r#""line\nbreak""#)]
	#[case("tab\there", r#""tab\there""#)]
	#[case("both '\" and \\", r#""both '\" and \\""#)]
	fn quote_uses_the_least_punctuation(#[case] value: &str, #[case] expected: &str) {
		assert_eq!(quote(value), expected);
	}

	/// Every value must survive a print-parse cycle, whichever form `quote` picked.
	#[rstest]
	#[case("value")]
	#[case("")]
	#[case("foo bar")]
	#[case("Grüße")]
	#[case("it's")]
	#[case(r#"it's a "quote""#)]
	#[case("line\nbreak")]
	#[case("tab\there")]
	#[case(r"back\slash")]
	#[case("carriage\rreturn")]
	#[case("[brackets], = pipes |")]
	#[case("# not a comment")]
	fn quote_round_trips_any_value(#[case] value: &str) {
		let node = VPLNode::from(("node", ("key", value)));
		let printed = VPLPipeline::from(node.clone()).to_string();
		assert_eq!(
			parse_vpl(&printed).unwrap(),
			VPLPipeline::from(node),
			"printed as: {printed}"
		);
	}

	#[rstest]
	#[case("node", "node")]
	#[case("node key=value", "node key=value")]
	// parameters come back alphabetised, and quoting is normalised
	#[case("node zebra=1 alpha=2", "node alpha=2 zebra=1")]
	#[case(r#"node key="value""#, "node key=value")]
	#[case("node key='a b'", "node key='a b'")]
	// a one-element list and a plain value are the same thing once parsed
	#[case("node key=[value]", "node key=value")]
	#[case("node key=[]", "node key=[]")]
	#[case(r#"node key=[a, "b c", '']"#, "node key=[a, 'b c', '']")]
	// repeated keys accumulate into one list
	#[case("node key=a key=b", "node key=[a, b]")]
	// sources, nested pipelines, and formatting normalisation
	#[case("node [ child ]", "node [ child ]")]
	#[case("node [a,b]", "node [ a, b ]")]
	#[case("node [ a | b, c ]", "node [ a | b, c ]")]
	// a value written with brackets must stay distinguishable from the sources that follow it
	#[case("node key=value [ child ]", "node key=value [ child ]")]
	#[case("node key=[] [ child ]", "node key=[] [ child ]")]
	#[case("node key=[a,b] [ child ]", "node key=[a, b] [ child ]")]
	#[case("a|b |\n c", "a | b | c")]
	#[case("# comment\nnode key=value # trailing", "node key=value")]
	#[case(
		"from_stacked [ from_container filename='a b' | filter level_min=5, from_container filename=c ] | raster_format format=webp",
		"from_stacked [ from_container filename='a b' | filter level_min=5, from_container filename=c ] | raster_format format=webp"
	)]
	fn pipeline_display(#[case] input: &str, #[case] expected: &str) {
		let pipeline = parse_vpl(input).unwrap();
		assert_eq!(pipeline.to_string(), expected);
		// and printing is idempotent: the output parses back to the same pipeline
		assert_eq!(parse_vpl(&pipeline.to_string()).unwrap(), pipeline);
	}

	#[test]
	fn node_display() {
		let node = VPLNode::try_from_str("node key1=value1 key2='a b' [ child ]").unwrap();
		assert_eq!(node.to_string(), "node key1=value1 key2='a b' [ child ]");
	}

	#[test]
	fn pipeline_round_trips_the_testdata_file() {
		const INPUT: &str = include_str!("../../../testdata/berlin.vpl");
		let pipeline = parse_vpl(INPUT).unwrap();
		assert_eq!(parse_vpl(&pipeline.to_string()).unwrap(), pipeline);
	}

	/// An empty pipeline has no spelling in the grammar; it prints as the empty string and
	/// does not parse back. Documented rather than fixed — see the module docs.
	#[test]
	fn empty_pipeline_prints_as_nothing() {
		assert_eq!(VPLPipeline::default().to_string(), "");
		assert!(parse_vpl("").is_err());
	}
}
