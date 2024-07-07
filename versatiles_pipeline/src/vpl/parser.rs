use super::{VPLNode, VPLPipeline};
use anyhow::{ensure, Context, Result};
use nom::{
	branch::alt,
	bytes::complete::{escaped_transform, tag, take_while, take_while1},
	character::complete::{alphanumeric1, char, multispace0, multispace1, none_of, one_of},
	combinator::{all_consuming, cut, opt, recognize, value},
	error::{context, convert_error, ContextError, VerboseError},
	multi::{many1, separated_list0, separated_list1},
	sequence::{delimited, pair, separated_pair, tuple},
	IResult, Parser,
};
use std::{collections::HashMap, fmt::Debug};

#[allow(dead_code)]
fn debug<I: Clone + Debug, E: ContextError<I>, F, O: Debug>(
	context: &'static str,
	mut f: F,
) -> impl FnMut(I) -> IResult<I, O, E>
where
	F: Parser<I, O, E>,
{
	move |i: I| {
		let result = f.parse(i.clone());
		println!("CONTEXT: {context}");
		println!("  INPUT: {i:?}");
		if let Ok(v) = &result {
			println!("  \x1b[0;32mRESULT: {v:?}\x1b[0m");
		} else {
			println!("  \x1b[0;31mERROR!!!\x1b[0m");
		}
		result
	}
}

fn parse_unquoted_value(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"unquoted value",
		recognize(many1(alt((alphanumeric1, recognize(one_of(".-_")))))),
	)(input)
	.map(|(a, b)| (a, b.to_string()))
}

fn parse_string(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"string",
		escaped_transform(
			none_of("\\\""),
			'\\',
			alt((
				value("\\", tag("\\")),
				value("\"", tag("\"")),
				value("\n", tag("n")),
				value("\t", tag("t")),
			)),
		),
	)(input)
}

fn parse_bare_identifier(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"parse_bare_identifier",
		recognize(pair(
			take_while1(|c: char| c.is_ascii_alphabetic()),
			take_while(|c: char| c.is_ascii_alphanumeric() || "_-".contains(c)),
		)),
	)(input)
	.map(|(a, b)| (a, b.to_string()))
}

fn parse_quoted_string(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"quoted string",
		delimited(char('\"'), parse_string, cut(char('\"'))),
	)(input)
}

fn parse_array(input: &str) -> IResult<&str, Vec<String>, VerboseError<&str>> {
	context(
		"array",
		delimited(
			tuple((char('['), multispace0)),
			separated_list0(
				tuple((multispace0, char(','), multispace0)),
				alt((parse_quoted_string, parse_unquoted_value)),
			),
			tuple((multispace0, char(']'))),
		),
	)(input)
}

fn parse_value(input: &str) -> IResult<&str, Vec<String>, VerboseError<&str>> {
	context(
		"value",
		alt((
			parse_quoted_string.map(|v| vec![v]),
			parse_unquoted_value.map(|v| vec![v]),
			parse_array,
		)),
	)(input)
}

fn parse_property(input: &str) -> IResult<&str, (String, Vec<String>), VerboseError<&str>> {
	context(
		"property",
		separated_pair(
			parse_identifier,
			cut(tuple((multispace0, char('='), multispace0))),
			cut(parse_value),
		),
	)(input)
}

fn parse_identifier(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context("node identifier", parse_bare_identifier)(input)
}

fn parse_sources(input: &str) -> IResult<&str, Vec<VPLPipeline>, VerboseError<&str>> {
	context(
		"sources",
		opt(delimited(
			tuple((char('['), multispace0)),
			separated_list0(char(','), cut(parse_pipeline)),
			tuple((multispace0, cut(char(']')))),
		))
		.map(|r| r.unwrap_or_default()),
	)(input)
}

fn parse_node<'a>(input: &'a str) -> IResult<&str, VPLNode, VerboseError<&str>> {
	context("node", |input: &'a str| {
		let (input, _) = multispace0(input)?;
		let (input, name) = cut(parse_identifier)(input)?;
		let (input, _) = multispace0(input)?;
		let (input, property_list) = separated_list0(multispace1, parse_property)(input)?;
		let (input, _) = multispace0(input)?;
		let (input, children) = parse_sources(input)?;
		let (input, _) = multispace0(input)?;

		let mut properties = HashMap::new();
		for (key, mut values) in property_list {
			properties
				.entry(key)
				.and_modify(|list: &mut Vec<String>| list.append(&mut values))
				.or_insert(values);
		}

		Ok((
			input,
			VPLNode {
				name,
				properties,
				sources: children,
			},
		))
	})(input)
}

fn parse_pipeline(input: &str) -> IResult<&str, VPLPipeline, VerboseError<&str>> {
	context(
		"pipeline",
		delimited(
			multispace0,
			separated_list1(char('|'), parse_node).map(VPLPipeline::new),
			multispace0,
		),
	)(input)
}

pub fn parse_vpl(input: &str) -> Result<VPLPipeline> {
	let result = all_consuming(parse_pipeline)(input);
	match result {
		Ok((leftover, pipeline)) => {
			ensure!(
				leftover.trim().is_empty(),
				"VPL didn't parse till the end. The rest: '{leftover}'"
			);
			Ok(pipeline)
		}
		Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
			Err(anyhow::anyhow!("{}", convert_error(input, e)))
		}
		Err(e) => {
			Err(anyhow::anyhow!("Error parsing VPL: {:?}", e)).context("Failed to parse VPL input")
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use lazy_static::lazy_static;
	use regex::{Regex, RegexBuilder};

	#[test]
	fn test_parse_bare_identifier() {
		for input in ["foo", "foo123", "foo-bar", "foo_bar"] {
			assert_eq!(parse_bare_identifier(input).unwrap().1, input)
		}

		for input in ["123foo", "=a", "-foo"] {
			let r = parse_bare_identifier(input);
			assert!(r.is_err());
		}
	}

	#[test]
	fn test_parse_identifier() {
		assert_eq!(parse_identifier("foo"), Ok(("", "foo".to_string())));
		assert!(parse_identifier("\"foo\"").is_err());
		assert!(parse_identifier("123foo").is_err());
		assert!(parse_identifier("\"foo").is_err());
	}

	#[test]
	fn test_parse_quoted_string() {
		assert_eq!(parse_quoted_string("\"foo\""), Ok(("", "foo".to_string())));
		assert_eq!(
			parse_quoted_string("\"foo bar\""),
			Ok(("", "foo bar".to_string()))
		);
		assert_eq!(
			parse_quoted_string("\"foo\\\"bar\\\"\""),
			Ok(("", "foo\"bar\"".to_string()))
		);
		assert!(parse_quoted_string("\"foo").is_err());
		assert!(parse_quoted_string("foo\"").is_err());
	}

	#[test]
	fn test_parse_prop() {
		let check = |a, b: &str, c: &str| {
			assert_eq!(
				parse_property(a),
				Ok(("", (b.to_string(), vec![c.to_string()]))),
				"error on: {a}"
			)
		};
		check("key=value", "key", "value");
		check("key=\"value\"", "key", "value");
		check("key=-2.0", "key", "-2.0");
	}

	#[test]
	fn test_parse_node() {
		let input = "node key1=value1 key2=\"value2\" key3=\"a=\\\"b\\\"\" [ child ]";
		let expected = VPLNode::from((
			"node",
			vec![("key1", "value1"), ("key2", "value2"), ("key3", "a=\"b\"")],
			VPLPipeline::from(VPLNode::from("child")),
		));
		assert_eq!(parse_node(input), Ok(("", expected)));
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
				"vectortiles_update_properties",
				vec![
					("data_source_path", "cities.csv"),
					("id_field_tiles", "id"),
					("id_field_data", "city_id"),
				],
			)),
		]);
		assert_eq!(parse_vpl(INPUT).unwrap(), expected);
	}

	#[test]
	fn test_parse_unquoted_value() {
		let inputs = ["value1", "value.1", "value-1", "value_1"];

		for input in inputs.iter() {
			assert_eq!(parse_unquoted_value(input).unwrap().1, *input);
		}
	}

	#[test]
	fn test_parse_value() {
		assert_eq!(parse_value("value1"), Ok(("", vec!["value1".to_string()])));
		assert_eq!(
			parse_value("\"value1\""),
			Ok(("", vec!["value1".to_string()]))
		);
		assert_eq!(
			parse_value("value 1"),
			Ok((" 1", vec!["value".to_string()]))
		);
		assert_eq!(
			parse_value("value\""),
			Ok(("\"", vec!["value".to_string()]))
		);
		assert!(parse_value("\"value").is_err());
	}

	#[test]
	fn test_error_messages() {
		lazy_static! {
			static ref REG_MGS1: Regex = RegexBuilder::new(r##"\s+"##).build().unwrap();
			static ref REG_MGS2: Regex = RegexBuilder::new(r##"\d+: at line \d+[,:]"##)
				.build()
				.unwrap();
		}

		fn run(vpl: &str, message: &str) {
			let mut error = parse_vpl(vpl)
				.unwrap_err()
				.to_string()
				.replace("^", " ")
				.replace("\n", " ");
			error = error.replace(vpl, "");
			error = REG_MGS1.replace_all(&error, " ").to_string();
			error = REG_MGS2
				.split(&error)
				.skip(1)
				.map(|l| l.trim().trim_end_matches(':'))
				.collect::<Vec<_>>()
				.join("; ");

			assert_eq!(error.trim(), message, "for vpl: '{vpl}'");
		}

		run("node [ child key=value ] node", "in Eof");
		run(
			"node child key=value ]",
			"expected '=', found k; in property; in node; in pipeline",
		);
		run("node key=\"2.1", "expected '\"', got end of input; in quoted string; in value; in property; in node; in pipeline");
		run("node [n key=2,1]", "in TakeWhile1; in parse_bare_identifier; in node identifier; in node; in pipeline; in sources; in node; in pipeline");
		run("node [n key=2]]", "in Eof");
		run("node [ ] [ ]", "in TakeWhile1; in parse_bare_identifier; in node identifier; in node; in pipeline; in sources; in node; in pipeline");
		run(
			"node [ a; b ]",
			"expected ']', found ;; in sources; in node; in pipeline",
		);
		run(
			"node | | node",
			"in TakeWhile1; in parse_bare_identifier; in node identifier; in node; in pipeline",
		);
	}
}
