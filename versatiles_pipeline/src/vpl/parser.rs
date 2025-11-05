use super::{VPLNode, VPLPipeline};
use anyhow::{Result, ensure};
use nom::{
	IResult, Parser,
	branch::alt,
	bytes::complete::{escaped_transform, tag, take_while, take_while1},
	character::complete::{alphanumeric1, char, multispace1, none_of, one_of},
	combinator::{all_consuming, cut, opt, recognize, value},
	error::context,
	multi::{many0, many1, separated_list0, separated_list1},
	sequence::{delimited, pair, preceded, separated_pair},
};
use nom_language::error::{VerboseError, convert_error};
use std::collections::BTreeMap;
use versatiles_derive::context;

// Consume whitespace **and** shell-style comments ("# ...\n").
fn comment(i: &str) -> IResult<&str, (), VerboseError<&str>> {
	value((), preceded(char('#'), take_while(|c: char| c != '\n'))).parse(i)
}

fn ws0(i: &str) -> IResult<&str, (), VerboseError<&str>> {
	value((), many0(alt((value((), multispace1), comment)))).parse(i)
}

fn ws1(i: &str) -> IResult<&str, (), VerboseError<&str>> {
	value((), many1(alt((value((), multispace1), comment)))).parse(i)
}

fn parse_unquoted_value(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"parsing unquoted value",
		recognize(many1(alt((alphanumeric1, recognize(one_of(".-_")))))),
	)
	.parse(input)
	.map(|(a, b)| (a, b.to_string()))
}

fn parse_string(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"parsing string",
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
	)
	.parse(input)
}

fn parse_bare_identifier(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"parsing bare_identifier",
		recognize(pair(
			take_while1(|c: char| c.is_ascii_alphabetic()),
			take_while(|c: char| c.is_ascii_alphanumeric() || "_-".contains(c)),
		)),
	)
	.parse(input)
	.map(|(a, b)| (a, b.to_string()))
}

fn parse_quoted_string(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context(
		"parsing quoted string",
		delimited(char('"'), parse_string, cut(char('"'))),
	)
	.parse(input)
}

fn parse_array(input: &str) -> IResult<&str, Vec<String>, VerboseError<&str>> {
	context(
		"parsing array",
		delimited(
			(char('['), ws0),
			separated_list0((ws0, char(','), ws0), alt((parse_quoted_string, parse_unquoted_value))),
			(ws0, char(']')),
		),
	)
	.parse(input)
}

fn parse_value(input: &str) -> IResult<&str, Vec<String>, VerboseError<&str>> {
	context(
		"parsing value",
		alt((
			parse_quoted_string.map(|v| vec![v]),
			parse_unquoted_value.map(|v| vec![v]),
			parse_array,
		)),
	)
	.parse(input)
}

fn parse_identifier(input: &str) -> IResult<&str, String, VerboseError<&str>> {
	context("parsing node identifier", parse_bare_identifier).parse(input)
}

fn parse_property(input: &str) -> IResult<&str, (String, Vec<String>), VerboseError<&str>> {
	context(
		"parsing property",
		separated_pair(parse_identifier, cut((ws0, char('='), ws0)), cut(parse_value)),
	)
	.parse(input)
}

fn parse_sources(input: &str) -> IResult<&str, Vec<VPLPipeline>, VerboseError<&str>> {
	context(
		"parsing sources",
		opt(delimited(
			(char('['), ws0),
			separated_list0((ws0, char(','), ws0), parse_pipeline),
			(ws0, cut(char(']'))),
		))
		.map(|r| r.unwrap_or_default()),
	)
	.parse(input)
}

fn parse_node<'a>(input: &'a str) -> IResult<&'a str, VPLNode, VerboseError<&'a str>> {
	context("parsing node", |input: &'a str| {
		let (input, _) = ws0(input)?;
		let (input, name) = parse_identifier(input)?;
		let (input, _) = ws0(input)?;
		let (input, property_list) = separated_list0(ws1, parse_property).parse(input)?;
		let (input, _) = ws0(input)?;
		let (input, children) = parse_sources(input)?;
		let (input, _) = ws0(input)?;

		let mut properties = BTreeMap::new();
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
	})
	.parse(input)
}

fn parse_pipeline(input: &str) -> IResult<&str, VPLPipeline, VerboseError<&str>> {
	context(
		"parsing pipeline",
		delimited(
			ws0,
			separated_list1((ws0, char('|'), ws0), parse_node).map(VPLPipeline::new),
			ws0,
		),
	)
	.parse(input)
}

#[context("Failed to parse VPL input")]
pub fn parse_vpl(input: &str) -> Result<VPLPipeline> {
	let result = all_consuming(parse_pipeline).parse(input);
	match result {
		Ok((leftover, pipeline)) => {
			ensure!(
				leftover.trim().is_empty(),
				"VPL didn't parse till the end. The rest: '{leftover}'"
			);
			Ok(pipeline)
		}
		Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => Err(anyhow::anyhow!(convert_error(input, e))),
		Err(e) => Err(anyhow::anyhow!("Error parsing VPL: {:?}", e)).context("Failed to parse VPL input"),
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
		assert_eq!(parse_quoted_string("\"foo bar\""), Ok(("", "foo bar".to_string())));
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
		assert_eq!(parse_value("\"value1\""), Ok(("", vec!["value1".to_string()])));
		assert_eq!(parse_value("value 1"), Ok((" 1", vec!["value".to_string()])));
		assert_eq!(parse_value("value\""), Ok(("\"", vec!["value".to_string()])));
		assert!(parse_value("\"value").is_err());
	}

	#[test]
	fn test_error_messages() {
		lazy_static! {
			static ref REG_MGS1: Regex = RegexBuilder::new(r##"\n+$"##).build().unwrap();
			static ref REG_MGS2: Regex = RegexBuilder::new(r##"\n+"##).build().unwrap();
		}

		fn run(vpl: &str, message: &str) {
			let mut error = parse_vpl(vpl).unwrap_err().to_string();
			error = REG_MGS1.replace_all(&error, "").to_string();
			error = REG_MGS2.replace_all(&error, "\n").to_string();
			assert_eq!(error, message, "for vpl: '{vpl}'");
		}

		run(
			"node [ child key=value ] node",
			"0: at line 1, in Eof:\nnode [ child key=value ] node\n                         ^",
		);
		run(
			"node child key=value ]",
			"0: at line 1:\nnode child key=value ]\n           ^\nexpected '=', found k\n1: at line 1, in parsing property:\nnode child key=value ]\n     ^\n2: at line 1, in parsing node:\nnode child key=value ]\n^\n3: at line 1, in parsing pipeline:\nnode child key=value ]\n^",
		);
		run(
			"node key=\"2.1",
			"0: at line 1:\nnode key=\"2.1\n             ^\nexpected '\"', got end of input\n1: at line 1, in parsing quoted string:\nnode key=\"2.1\n         ^\n2: at line 1, in parsing value:\nnode key=\"2.1\n         ^\n3: at line 1, in parsing property:\nnode key=\"2.1\n     ^\n4: at line 1, in parsing node:\nnode key=\"2.1\n^\n5: at line 1, in parsing pipeline:\nnode key=\"2.1\n^",
		);
		run(
			"node [n key=2,1]",
			"0: at line 1:\nnode [n key=2,1]\n             ^\nexpected ']', found ,\n1: at line 1, in parsing sources:\nnode [n key=2,1]\n     ^\n2: at line 1, in parsing node:\nnode [n key=2,1]\n^\n3: at line 1, in parsing pipeline:\nnode [n key=2,1]\n^",
		);
		run(
			"node [n key=2]]",
			"0: at line 1, in Eof:\nnode [n key=2]]\n              ^",
		);
		run("node [ ] [ ]", "0: at line 1, in Eof:\nnode [ ] [ ]\n         ^");
		run(
			"node [ a; b ]",
			"0: at line 1:\nnode [ a; b ]\n        ^\nexpected ']', found ;\n1: at line 1, in parsing sources:\nnode [ a; b ]\n     ^\n2: at line 1, in parsing node:\nnode [ a; b ]\n^\n3: at line 1, in parsing pipeline:\nnode [ a; b ]\n^",
		);
		run("node | | node", "0: at line 1, in Eof:\nnode | | node\n     ^");
	}
}
