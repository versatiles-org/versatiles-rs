//! The lossless parser: VPL text in, [`CstFile`] out.
//!
//! This is a port of [`parser`](super::parser) that keeps what the semantic grammar discards.
//! Two things are deliberately identical to it, because both are observable:
//!
//! - the `context(…)` labels and `cut(…)` placement, which the rendered error traces are built
//!   from and which eight tests pin byte-for-byte;
//! - what the grammar *accepts*. In particular parameters are still separated by `ws1`: with
//!   `ws0` there, `node a="x"b=2` would silently become two parameters instead of an error.
//!
//! # Trivia
//!
//! Each token consumes the whitespace and comments immediately before it, so trivia lands in
//! exactly one place. Anything left at the end becomes [`CstFile::trailing`]. Nothing is consumed
//! twice, and nothing is dropped, which is what makes printing exact.
//!
//! # Spans
//!
//! Offsets are assigned after parsing rather than during it. Because the tree is lossless, walking
//! it in order and accumulating `leading.len() + text.len()` reproduces the source position of
//! every token — and the total must come out equal to the input length, which makes the walk its
//! own check that nothing was lost.

use super::{
	CstArray, CstFile, CstNode, CstPipeline, CstProperty, CstSources, CstString, CstStringKind, CstToken, CstValue,
	Punctuated, PunctuatedItem, VplParseError,
};
use nom::{
	IResult, Parser,
	branch::alt,
	bytes::complete::{escaped, tag, take_while, take_while1},
	character::complete::{alphanumeric1, char, multispace1, none_of, one_of},
	combinator::{all_consuming, cut, peek, recognize, value},
	error::context,
	multi::{many0, many1},
	sequence::{pair, preceded},
};
use nom_language::error::VerboseError;

type Res<'a, T> = IResult<&'a str, T, VerboseError<&'a str>>;

// ---------------------------------------------------------------------------------------------
// Trivia
// ---------------------------------------------------------------------------------------------

fn comment(i: &str) -> Res<'_, &str> {
	recognize(preceded(char('#'), take_while(|c: char| c != '\n'))).parse(i)
}

fn ws0(i: &str) -> Res<'_, &str> {
	recognize(many0(alt((multispace1, comment)))).parse(i)
}

fn ws1(i: &str) -> Res<'_, &str> {
	recognize(many1(alt((multispace1, comment)))).parse(i)
}

/// Reads optional trivia followed by `inner`, packaging both as one token.
fn token0<'a>(input: &'a str, inner: impl Fn(&'a str) -> Res<'a, &'a str>) -> Res<'a, CstToken> {
	let (input, leading) = ws0(input)?;
	let (input, text) = inner(input)?;
	Ok((input, CstToken::with_leading(leading, text)))
}

/// Reads optional trivia followed by a single expected character.
fn punct(input: &str, expected: char) -> Res<'_, CstToken> {
	token0(input, |i| recognize(char(expected)).parse(i))
}

// ---------------------------------------------------------------------------------------------
// Leaves
// ---------------------------------------------------------------------------------------------

fn bare_identifier(input: &str) -> Res<'_, &str> {
	context(
		"parsing bare_identifier",
		recognize(pair(
			take_while1(|c: char| c.is_ascii_alphabetic()),
			take_while(|c: char| c.is_ascii_alphanumeric() || "_-".contains(c)),
		)),
	)
	.parse(input)
}

fn identifier(input: &str) -> Res<'_, &str> {
	context("parsing node identifier", bare_identifier).parse(input)
}

fn unquoted_value(input: &str) -> Res<'_, &str> {
	context(
		"parsing unquoted value",
		recognize(many1(alt((alphanumeric1, recognize(one_of(".-_")))))),
	)
	.parse(input)
}

/// Recognises a double-quoted string without decoding it.
///
/// The escape alternatives are spelled out as separate `tag`s rather than as a single `one_of`
/// so that a bad escape produces the same `nom` frames — and so the same rendered trace — as the
/// semantic parser's `escaped_transform`.
fn double_quoted(input: &str) -> Res<'_, &str> {
	context(
		"parsing double quoted string",
		recognize((
			char('"'),
			alt((
				// `escaped` cannot match an empty body, so `""` is handled separately.
				value("", peek(char('"'))),
				escaped(none_of("\\\""), '\\', alt((tag("\\"), tag("\""), tag("n"), tag("t")))),
			)),
			char('"'),
		)),
	)
	.parse(input)
}

fn single_quoted(input: &str) -> Res<'_, &str> {
	context(
		"parsing single quoted string",
		recognize((char('\''), take_while(|c: char| c != '\''), char('\''))),
	)
	.parse(input)
}

/// Reads a value that has already had its leading trivia consumed.
fn cst_string_at<'a>(input: &'a str, leading: &'a str) -> Res<'a, CstString> {
	let (rest, text) = if input.starts_with('"') {
		double_quoted(input)?
	} else if input.starts_with('\'') {
		single_quoted(input)?
	} else {
		unquoted_value(input)?
	};

	let kind = match text.chars().next() {
		Some('"') => CstStringKind::Double,
		Some('\'') => CstStringKind::Single,
		_ => CstStringKind::Bare,
	};
	Ok((
		rest,
		CstString {
			token: CstToken::with_leading(leading, text),
			kind,
		},
	))
}

/// Reads a value together with any trivia before it.
fn cst_string(input: &str) -> Res<'_, CstString> {
	let (rest, leading) = ws0(input)?;
	cst_string_at(rest, leading)
}

// ---------------------------------------------------------------------------------------------
// Composites
// ---------------------------------------------------------------------------------------------

// Note on trivia and `context(…)`: each composite below takes its leading trivia as an argument
// rather than reading it itself, so that its context frame is recorded at the construct's first
// real character. Reading the whitespace inside the frame would place the frame — and so the
// caret in the rendered trace — one token to the left of where the semantic parser puts it.

fn cst_array<'a>(input: &'a str, leading: &'a str) -> Res<'a, CstArray> {
	context("parsing array", move |input: &'a str| {
		let (input, open) = bracket(input, leading, '[')?;
		let (input, items) = punctuated(input, ',', cst_string)?;
		let (input, close) = punct(input, ']')?;
		Ok((input, CstArray { open, items, close }))
	})
	.parse(input)
}

/// A bracket token whose leading trivia was consumed by the caller.
fn bracket<'a>(input: &'a str, leading: &'a str, expected: char) -> Res<'a, CstToken> {
	let (input, text) = recognize(char(expected)).parse(input)?;
	Ok((input, CstToken::with_leading(leading, text)))
}

/// Reads `element (sep element)*`, or nothing at all, keeping every separator.
///
/// Mirrors `separated_list0`: if the element after a separator fails softly, the separator is
/// given back and the list ends there. Without that, `node [n key=2,1]` would report against the
/// `1` instead of against the comma.
fn punctuated<'a, T>(
	input: &'a str,
	separator: char,
	element: impl Fn(&'a str) -> Res<'a, T>,
) -> Res<'a, Punctuated<T>> {
	let mut items = Vec::new();
	let mut input = input;

	match element(input) {
		Ok((rest, value)) => {
			items.push(PunctuatedItem { separator: None, value });
			input = rest;
		}
		Err(nom::Err::Error(_)) => return Ok((input, Punctuated { items })),
		Err(e) => return Err(e),
	}

	while let Ok((rest, sep)) = punct(input, separator) {
		match element(rest) {
			Ok((rest, value)) => {
				items.push(PunctuatedItem {
					separator: Some(sep),
					value,
				});
				input = rest;
			}
			Err(nom::Err::Error(_)) => break,
			Err(e) => return Err(e),
		}
	}

	Ok((input, Punctuated { items }))
}

fn cst_property<'a>(input: &'a str, leading: &'a str) -> Res<'a, CstProperty> {
	context("parsing property", move |input: &'a str| {
		let (input, name) = identifier(input)?;
		let key = CstToken::with_leading(leading, name);
		let (input, equals) = cut(|i| punct(i, '=')).parse(input)?;
		let (input, value) = cut(cst_value).parse(input)?;
		Ok((input, CstProperty { key, equals, value }))
	})
	.parse(input)
}

fn cst_value(input: &str) -> Res<'_, CstValue> {
	let (rest, leading) = ws0(input)?;
	if rest.starts_with('[') {
		cst_array(rest, leading).map(|(i, array)| (i, CstValue::Array(array)))
	} else {
		cst_string_at(rest, leading).map(|(i, string)| (i, CstValue::Single(string)))
	}
}

fn cst_sources<'a>(input: &'a str, leading: &'a str) -> Res<'a, CstSources> {
	context("parsing sources", move |input: &'a str| {
		let (input, open) = bracket(input, leading, '[')?;
		let (input, pipelines) = punctuated(input, ',', cst_pipeline)?;
		let (input, close) = cut(|i| punct(i, ']')).parse(input)?;
		Ok((input, CstSources { open, pipelines, close }))
	})
	.parse(input)
}

fn cst_node<'a>(input: &'a str) -> Res<'a, CstNode> {
	context("parsing node", |input: &'a str| {
		let (input, name) = token0(input, identifier)?;

		let mut properties = Vec::new();
		let mut input = input;
		loop {
			// The first parameter only has to be separated from the node name, which the greedy
			// identifier already guarantees; later ones need real whitespace between them, or
			// `node a="x"b=2` would quietly become two parameters instead of an error.
			let separator = if properties.is_empty() { ws0 } else { ws1 };
			let Ok((rest, leading)) = separator(input) else {
				break;
			};
			match cst_property(rest, leading) {
				Ok((rest, property)) => {
					properties.push(property);
					input = rest;
				}
				// A soft error means there is simply no further parameter, and the trivia we read
				// belongs to whatever comes next. A `Failure` means a parameter was started and
				// then went wrong, and must not be swallowed by stopping here.
				Err(nom::Err::Error(_)) => break,
				Err(e) => return Err(e),
			}
		}

		let (after_ws, leading) = ws0(input)?;
		let (input, sources) = match cst_sources(after_ws, leading) {
			Ok((rest, sources)) => (rest, Some(sources)),
			Err(nom::Err::Error(_)) => (input, None),
			Err(e) => return Err(e),
		};

		Ok((
			input,
			CstNode {
				name,
				properties,
				sources,
			},
		))
	})
	.parse(input)
}

fn cst_pipeline<'a>(input: &'a str) -> Res<'a, CstPipeline> {
	context("parsing pipeline", |input: &'a str| {
		let (input, first) = cst_node(input)?;
		let mut nodes = vec![PunctuatedItem {
			separator: None,
			value: first,
		}];
		let mut input = input;

		// As `separated_list1`: a soft failure after the `|` gives the separator back, so
		// `node | | node` is reported as unexpected input at the first `|`.
		while let Ok((rest, pipe)) = punct(input, '|') {
			match cst_node(rest) {
				Ok((rest, node)) => {
					nodes.push(PunctuatedItem {
						separator: Some(pipe),
						value: node,
					});
					input = rest;
				}
				Err(nom::Err::Error(_)) => break,
				Err(e) => return Err(e),
			}
		}

		Ok((
			input,
			CstPipeline {
				nodes: Punctuated { items: nodes },
			},
		))
	})
	.parse(input)
}

fn cst_file(input: &str) -> Res<'_, CstFile> {
	let (input, pipeline) = cst_pipeline(input)?;
	let (input, trailing) = ws0(input)?;
	Ok((
		input,
		CstFile {
			pipeline,
			trailing: trailing.to_string(),
		},
	))
}

// ---------------------------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------------------------

/// Parses VPL text into a lossless syntax tree.
///
/// The result prints back to `input` byte for byte, keeping comments, whitespace, the order the
/// parameters were written in, and which quotes the author chose. Use
/// [`lower`](CstFile::lower) to get the semantic [`VPLPipeline`](super::VPLPipeline) when it is
/// time to run the pipeline.
///
/// # Examples
///
/// ```
/// use versatiles_pipeline::vpl::parse_cst;
///
/// let source = "# where the data lives\nfrom_container  filename='berlin.versatiles'\n";
/// let file = parse_cst(source).unwrap();
///
/// // nothing is lost: printing reproduces the source exactly
/// assert_eq!(file.to_string(), source);
///
/// // and every token knows where it came from
/// let node = &file.pipeline.nodes.items[0].value;
/// assert_eq!(&source[node.name.span.clone().unwrap()], "from_container");
/// ```
pub fn parse_cst(input: &str) -> Result<CstFile, VplParseError> {
	match all_consuming(cst_file).parse(input) {
		Ok((_, mut file)) => {
			let end = assign_spans(&mut file);
			debug_assert_eq!(end, input.len(), "CST lost text while parsing: {input:?}");
			Ok(file)
		}
		Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(VplParseError::from_verbose(input, &e)),
		Err(e) => Err(VplParseError::at_end_of_input(
			input,
			format!("Error parsing VPL: {e:?}"),
		)),
	}
}

// ---------------------------------------------------------------------------------------------
// Span assignment
// ---------------------------------------------------------------------------------------------

/// Walks the tree in source order, filling in every token's span. Returns the final offset, which
/// equals the input length exactly when nothing was lost.
fn assign_spans(file: &mut CstFile) -> usize {
	let mut offset = 0;
	walk_pipeline(&mut file.pipeline, &mut offset);
	offset + file.trailing.len()
}

fn walk_token(token: &mut CstToken, offset: &mut usize) {
	*offset += token.leading.len();
	let start = *offset;
	*offset += token.text.len();
	token.span = Some(start..*offset);
}

fn walk_pipeline(pipeline: &mut CstPipeline, offset: &mut usize) {
	for item in &mut pipeline.nodes.items {
		if let Some(separator) = &mut item.separator {
			walk_token(separator, offset);
		}
		walk_node(&mut item.value, offset);
	}
}

fn walk_node(node: &mut CstNode, offset: &mut usize) {
	walk_token(&mut node.name, offset);
	for property in &mut node.properties {
		walk_token(&mut property.key, offset);
		walk_token(&mut property.equals, offset);
		walk_value(&mut property.value, offset);
	}
	if let Some(sources) = &mut node.sources {
		walk_token(&mut sources.open, offset);
		for item in &mut sources.pipelines.items {
			if let Some(separator) = &mut item.separator {
				walk_token(separator, offset);
			}
			walk_pipeline(&mut item.value, offset);
		}
		walk_token(&mut sources.close, offset);
	}
}

fn walk_value(value: &mut CstValue, offset: &mut usize) {
	match value {
		CstValue::Single(string) => walk_token(&mut string.token, offset),
		CstValue::Array(array) => {
			walk_token(&mut array.open, offset);
			for item in &mut array.items.items {
				if let Some(separator) = &mut item.separator {
					walk_token(separator, offset);
				}
				walk_token(&mut item.value.token, offset);
			}
			walk_token(&mut array.close, offset);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vpl::parse_vpl;
	use rstest::rstest;

	/// Everything the semantic parser's own tests throw at it, plus the awkward shapes.
	const CORPUS: &[&str] = &[
		"node",
		"node key=value",
		"node key=\"value\"",
		"node key='value'",
		"node key=''",
		"node key=\"\"",
		"node key=-2.0",
		"node key=[]",
		"node key=[a]",
		"node key=[a, 'b c', \"d\"]",
		"node key=a key=b",
		"node1 key1=value1|\nnode2 key2=\"value2\"",
		"node1 key1=value1|node2 key2=\"value2\"| node3 key3=value3 |\nnode4 key4=value4",
		"node1 key1=value1 [ child1 key2=value2 | child2 key3=\"value3\", child3 key4=value4 ] | node2",
		"node [ child ]",
		"node [ ]",
		"node [a,b]",
		"node key=[] [ child ]",
		"  node  key = value  ",
		"# leading comment\nnode key=value # trailing comment\n",
		"node\n\tkey=value\n\tother=2",
		"node key=\"a\\nb\\tc\\\\d\\\"e\"",
		"node key='C:\\dir'",
		"node key='Grüße'",
		"from_container filename=\"berlin.versatiles\" | vector_update_properties data_source_path=\"cities.csv\"",
	];

	/// The defining property: whatever went in comes back out unchanged.
	#[rstest]
	fn round_trips_byte_for_byte(#[values(0)] _dummy: usize) {
		for source in CORPUS {
			let file = parse_cst(source).unwrap_or_else(|e| panic!("failed on {source:?}:\n{e}"));
			assert_eq!(&file.to_string(), source, "round trip differed for {source:?}");
		}
	}

	/// The lossless tree and the semantic parser must agree about what the text *means*.
	#[test]
	fn lowering_matches_the_semantic_parser() {
		for source in CORPUS {
			let cst = parse_cst(source).unwrap_or_else(|e| panic!("failed on {source:?}:\n{e}"));
			let expected = parse_vpl(source).unwrap_or_else(|e| panic!("failed on {source:?}:\n{e:#}"));
			assert_eq!(cst.lower(), expected, "lowering differed for {source:?}");
		}
	}

	/// The testdata file the semantic parser is tested against, for a realistic sample.
	#[test]
	fn round_trips_the_testdata_file() {
		const INPUT: &str = include_str!("../../../testdata/berlin.vpl");
		let file = parse_cst(INPUT).unwrap();
		assert_eq!(file.to_string(), INPUT);
		assert_eq!(file.lower(), parse_vpl(INPUT).unwrap());
	}

	/// Spans have to address the original text, not a reconstruction of it.
	#[test]
	fn spans_point_into_the_source() {
		let source = "# a comment\nfrom_container  filename = 'berlin.versatiles'  # tail\n";
		let file = parse_cst(source).unwrap();
		let node = &file.pipeline.nodes.items[0].value;

		assert_eq!(&source[node.name.span.clone().unwrap()], "from_container");
		let property = &node.properties[0];
		assert_eq!(&source[property.key.span.clone().unwrap()], "filename");
		assert_eq!(&source[property.equals.span.clone().unwrap()], "=");
		let super::CstValue::Single(value) = &property.value else {
			panic!("expected a single value")
		};
		assert_eq!(&source[value.token.span.clone().unwrap()], "'berlin.versatiles'");
		assert_eq!(value.decode(), "berlin.versatiles");
	}

	/// Nested pipelines get spans too, and the trailing comment stays out of the tree proper.
	#[test]
	fn spans_survive_nesting() {
		let source = "from_stacked [ a key=1, b ] | filter level_min=5";
		let file = parse_cst(source).unwrap();
		let sources = file.pipeline.nodes.items[0].value.sources.as_ref().unwrap();

		let first = &sources.pipelines.items[0].value.nodes.items[0].value;
		assert_eq!(&source[first.name.span.clone().unwrap()], "a");
		let second = &sources.pipelines.items[1].value.nodes.items[0].value;
		assert_eq!(&source[second.name.span.clone().unwrap()], "b");

		let filter = &file.pipeline.nodes.items[1].value;
		assert_eq!(&source[filter.name.span.clone().unwrap()], "filter");
	}

	/// Parameters keep the order and the duplicates the semantic tree throws away.
	#[test]
	fn parameters_stay_in_source_order() {
		let file = parse_cst("node zebra=1 alpha=2 zebra=3").unwrap();
		let node = &file.pipeline.nodes.items[0].value;

		let keys: Vec<&str> = node.properties.iter().map(|p| p.key.text.as_str()).collect();
		assert_eq!(keys, ["zebra", "alpha", "zebra"]);
		// while lowering merges and alphabetises, exactly as before
		assert_eq!(file.lower(), parse_vpl("node zebra=1 alpha=2 zebra=3").unwrap());
	}

	/// Both parsers must reject the same inputs *and* say the same thing about them. The trace is
	/// observable output, so an identical rejection with a different message is still a change.
	#[rstest]
	#[case("")]
	#[case("node child key=value ]")]
	#[case("node key=\"2.1")]
	#[case("node key='2.1")]
	#[case("node [n key=2,1]")]
	#[case("node [n key=2]]")]
	#[case("node [ ] [ ]")]
	#[case("node [ a; b ]")]
	#[case("node | | node")]
	#[case("node [ child key=value ] node")]
	#[case("node [")]
	#[case("node key=")]
	#[case("node key")]
	#[case("123")]
	#[case("node a=\"foo\\qbar\"")]
	#[case("node a=!")]
	#[case("node a=ä")]
	#[case("node a=[1 2]")]
	#[case("no de")]
	// the divergence a `ws0` between parameters would have introduced
	#[case("node a=\"x\"b=2")]
	fn fails_exactly_like_the_semantic_parser(#[case] source: &str) {
		let semantic = parse_vpl(source).expect_err("semantic parser accepted it");
		let semantic = semantic.chain().last().unwrap().to_string();
		let cst = parse_cst(source).expect_err("CST parser accepted it").to_string();
		assert_eq!(cst, semantic, "trace differed for {source:?}");
	}
}
