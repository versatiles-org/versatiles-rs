//! VPL serialisation.
//!
//! The counterpart to [`parser`](super::parser): where the parser turns VPL text into a
//! [`VPLPipeline`], the [`Display`] implementations here turn one back into text that the
//! parser accepts. That makes it possible to write a `.vpl` file from a pipeline assembled
//! in code, and to round-trip the parser against itself in tests.
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

use super::{VPLNode, VPLPipeline};
use std::{
	borrow::Cow,
	fmt::{self, Display, Write},
};

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
fn quote(value: &str) -> Cow<'_, str> {
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

impl Display for VPLNode {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.name)?;

		for (key, values) in &self.properties {
			write!(f, " {key}=")?;
			// A single value is written plainly; anything else needs the bracket form, which
			// includes the empty list, since `a=` alone is not a value.
			if let [value] = values.as_slice() {
				f.write_str(&quote(value))?;
			} else {
				f.write_char('[')?;
				for (i, value) in values.iter().enumerate() {
					if i > 0 {
						f.write_str(", ")?;
					}
					f.write_str(&quote(value))?;
				}
				f.write_char(']')?;
			}
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
mod tests {
	use super::*;
	use crate::vpl::parse_vpl;
	use rstest::rstest;

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
