use std::fmt::Debug;

use anyhow::{bail, Context, Result};
use nom::{
	branch::alt,
	bytes::complete::{escaped_transform, tag, take_till, take_until, take_while, take_while1},
	character::complete::{alphanumeric1, char, line_ending, none_of, one_of, space1},
	combinator::{eof, map, opt, recognize, value},
	error::{context, ContextError, ParseError},
	multi::{many1, separated_list0},
	sequence::{delimited, pair, terminated},
	IResult, Parser,
};

// Based on https://github.com/kdl-org/kdl/blob/main/SPEC.md#full-grammar

#[derive(Debug, PartialEq)]
pub struct KDLNode {
	pub name: String,
	pub properties: Vec<(String, String)>,
	pub children: Vec<KDLNode>,
}

impl KDLNode {
	pub fn get_property(&self, key: &str) -> Option<&String> {
		self
			.properties
			.iter()
			.find(|(k, _)| k == key)
			.map(|(_, v)| v)
	}
}

fn parse_unquoted_value<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, String, E> {
	context(
		"parse_unquoted_value",
		recognize(many1(alt((alphanumeric1, recognize(one_of(".-_")))))),
	)(input)
	.map(|(a, b)| (a, b.to_string()))
}

fn parse_string<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, String, E> {
	context(
		"parse_string",
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

fn is_initial_identifier_char(c: char) -> bool {
	!c.is_ascii_digit() && is_identifier_char(c)
}

fn is_identifier_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || "_-".contains(c)
}

fn parse_bare_identifier<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, String, E> {
	context(
		"parse_bare_identifier",
		recognize(pair(
			take_while1(|c: char| is_initial_identifier_char(c)),
			take_while(|c: char| is_identifier_char(c)),
		)),
	)(input)
	.map(|(a, b)| (a, b.to_string()))
}

fn parse_quoted_string<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, String, E> {
	context(
		"parse_quoted_string",
		delimited(char('\"'), parse_string, char('\"')),
	)(input)
}

fn parse_value<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, String, E> {
	context(
		"parse_value",
		alt((parse_quoted_string, parse_unquoted_value)),
	)(input)
}

fn parse_prop<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, (String, String), E> {
	context("parse_prop", |input: &'a str| {
		let (input, key) = parse_identifier(input)?;
		let (input, _) = char('=')(input)?;
		let (input, value) = parse_value(input)?;

		Ok((input, (key.to_string(), value.to_string())))
	})(input)
}

fn parse_identifier<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, String, E> {
	context(
		"parse_identifier",
		alt((parse_quoted_string, parse_bare_identifier)),
	)(input)
}

fn parse_node_space<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &'a str, E> {
	context("parse_node_space", parse_ws)(input)
}

fn parse_node_terminator<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &'a str, E> {
	context(
		"parse_node_terminator",
		recognize(alt((parse_single_line_comment, line_ending, tag(";"), eof))),
	)(input)
}

fn parse_node<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, Option<KDLNode>, E> {
	context("parse_node", |input: &'a str| {
		let (input, _) = opt(parse_linespace)(input)?;
		let (input, _) = opt(parse_node_space)(input)?;
		let (input, name) = parse_identifier(input)?;
		let (input, _) = opt(parse_node_space)(input)?;
		let (input, properties) = separated_list0(parse_node_space, parse_prop)(input)?;
		let (input, _) = opt(parse_node_space)(input)?;
		let (input, children) = parse_children(input)?;
		let (input, _) = opt(parse_node_space)(input)?;

		Ok((
			input,
			Some(KDLNode {
				name,
				properties,
				children,
			}),
		))
	})(input)
}

fn parse_multi_line_comment<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	context(
		"parse_multi_line_comment",
		delimited(tag("/*"), take_until("*/"), tag("*/")),
	)(input)
}

fn parse_single_line_comment<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	context(
		"parse_single_line_comment",
		recognize(pair(tag("//"), take_till(|c| c == '\n'))),
	)(input)
}

fn parse_ws<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	context(
		"parse_ws",
		recognize(many1(alt((space1, parse_multi_line_comment)))),
	)(input)
}

fn parse_linespace<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &'a str, E> {
	context(
		"parse_linespace",
		recognize(alt((line_ending, parse_ws, parse_single_line_comment))),
	)(input)
}

#[allow(dead_code)]
fn debug<I: Clone + Debug, E: ContextError<I>, F, O: Debug>(
	mut f: F,
	context: &'static str,
) -> impl FnMut(I) -> IResult<I, O, E>
where
	F: Parser<I, O, E>,
{
	move |i: I| {
		let result = f.parse(i.clone());
		println!("CONTEXT: {context}");
		println!("INPUT: {i:?}");
		if let Ok(v) = &result {
			println!("RESULT: {v:?}");
		} else {
			println!("ERROR!!!");
		}
		result
	}
}

fn parse_node_list<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, Vec<KDLNode>, E> {
	context(
		"parse_node_list",
		separated_list0(
			many1(parse_node_terminator),
			alt((parse_node, map(parse_linespace, |_| None))),
		)
		.map(|v| v.into_iter().filter_map(|e| e).collect::<Vec<_>>()),
	)(input)
}

fn parse_children<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, Vec<KDLNode>, E> {
	if let Ok(rest) = char::<&'a str, E>('{')(input) {
		context("parse_children", terminated(parse_node_list, char('}')))(rest.0)
	} else {
		Ok((input, Vec::new()))
	}
}

pub fn parse_kdl(input: &str) -> Result<Vec<KDLNode>> {
	match parse_node_list(input) {
		Ok((leftover, nodes)) => {
			if leftover.trim().is_empty() {
				Ok(nodes)
			} else {
				bail!("KDL didn't parse till the end: '{leftover}'")
			}
		}
		Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
			let err_msg = nom::error::convert_error(input, e);
			Err(anyhow::anyhow!("Error parsing KDL: {}", err_msg)).context("Failed to parse KDL input")
		}
		Err(e) => {
			Err(anyhow::anyhow!("Error parsing KDL: {:?}", e)).context("Failed to parse KDL input")
		}
	}
}

#[cfg(test)]
mod tests {
	use nom::{error::VerboseError, multi::many0, InputIter};

	use super::*;

	type V = VerboseError<&'static str>;

	#[test]
	fn test_is_identifier_char() {
		for c in "abcxyzABCXYZ0123456789-_".iter_elements() {
			assert!(is_identifier_char(c));
		}
		for c in "!?\"\\. \t\n".iter_elements() {
			assert!(!is_identifier_char(c));
		}
	}

	#[test]
	fn test_parse_bare_identifier() {
		for (input, output) in [
			("foo", "foo"),
			("foo123", "foo123"),
			("-foo", "-foo"),
			("foo-bar", "foo-bar"),
			("foo_bar", "foo_bar"),
			("foo!bar", "foo"),
		] {
			assert_eq!(parse_bare_identifier::<V>(input).unwrap().1, output)
		}

		for input in ["123foo", "=a"] {
			let r = parse_bare_identifier::<V>(input);
			assert!(r.is_err(), "input did not fail: {input}");
		}
	}

	#[test]
	fn test_parse_identifier() {
		assert_eq!(parse_identifier::<V>("foo"), Ok(("", "foo".to_string())));
		assert_eq!(
			parse_identifier::<V>("\"foo\""),
			Ok(("", "foo".to_string()))
		);
		assert!(parse_identifier::<V>("123foo").is_err());
		assert!(parse_identifier::<V>("\"foo").is_err());
	}

	#[test]
	fn test_parse_quoted_string() {
		assert_eq!(
			parse_quoted_string::<V>("\"foo\""),
			Ok(("", "foo".to_string()))
		);
		assert_eq!(
			parse_quoted_string::<V>("\"foo bar\""),
			Ok(("", "foo bar".to_string()))
		);
		assert_eq!(
			parse_quoted_string::<V>("\"foo\\\"bar\\\"\""),
			Ok(("", "foo\"bar\"".to_string()))
		);
		assert!(parse_quoted_string::<V>("\"foo").is_err());
		assert!(parse_quoted_string::<V>("foo\"").is_err());
	}

	#[test]
	fn test_parse_prop() {
		let check = |a, b: &str, c: &str| {
			assert_eq!(
				parse_prop::<V>(a),
				Ok(("", (b.to_string(), c.to_string()))),
				"error on: {a}"
			)
		};
		check("key=value", "key", "value");
		check("key=\"value\"", "key", "value");
		check("key=-2.0", "key", "-2.0");
	}

	#[test]
	fn test_parse_line_comment() {
		assert_eq!(
			parse_single_line_comment::<V>("// comment\nrest").unwrap(),
			("\nrest", "// comment")
		);
	}

	#[test]
	fn test_parse_node() {
		let input = "node key1=value1 key2=\"value2\" key3=\"a=\\\"b\\\"\" { child }";
		let expected = KDLNode {
			name: "node".to_string(),
			properties: vec![
				("key1".to_string(), "value1".to_string()),
				("key2".to_string(), "value2".to_string()),
				("key3".to_string(), "a=\"b\"".to_string()),
			],
			children: vec![KDLNode {
				name: "child".to_string(),
				properties: vec![],
				children: vec![],
			}],
		};
		assert_eq!(parse_node::<V>(input), Ok(("", Some(expected))));
	}

	#[test]
	fn test_parse_nodes1() {
		let input = "node1 key1=value1\nnode2 key2=\"value2\"";
		let expected = vec![
			KDLNode {
				name: "node1".to_string(),
				properties: vec![("key1".to_string(), "value1".to_string())],
				children: vec![],
			},
			KDLNode {
				name: "node2".to_string(),
				properties: vec![("key2".to_string(), "value2".to_string())],
				children: vec![],
			},
		];
		assert_eq!(parse_kdl(input).unwrap(), expected);
	}

	#[test]
	fn test_parse_nodes2() {
		let input = "node1 key1=value1;node2 key2=\"value2\";node3 key3=value3\nnode4 key4=value4";
		let expected = vec![
			KDLNode {
				name: "node1".to_string(),
				properties: vec![("key1".to_string(), "value1".to_string())],
				children: vec![],
			},
			KDLNode {
				name: "node2".to_string(),
				properties: vec![("key2".to_string(), "value2".to_string())],
				children: vec![],
			},
			KDLNode {
				name: "node3".to_string(),
				properties: vec![("key3".to_string(), "value3".to_string())],
				children: vec![],
			},
			KDLNode {
				name: "node4".to_string(),
				properties: vec![("key4".to_string(), "value4".to_string())],
				children: vec![],
			},
		];
		assert_eq!(parse_kdl(input).unwrap(), expected);
	}

	#[test]
	fn test_parse_nodes3() {
		let input = "node1 key1=value1 { child1 key2=value2; child2 key3=\"value3\"; }; node2";
		let expected = vec![
			KDLNode {
				name: "node1".to_string(),
				properties: vec![("key1".to_string(), "value1".to_string())],
				children: vec![
					KDLNode {
						name: "child1".to_string(),
						properties: vec![("key2".to_string(), "value2".to_string())],
						children: vec![],
					},
					KDLNode {
						name: "child2".to_string(),
						properties: vec![("key3".to_string(), "value3".to_string())],
						children: vec![],
					},
				],
			},
			KDLNode {
				name: "node2".to_string(),
				properties: vec![],
				children: vec![],
			},
		];
		assert_eq!(parse_kdl(input).unwrap(), expected);
	}

	#[test]
	fn test_parse_unquoted_value() {
		let inputs = ["value1", "value.1", "value-1", "value_1"];

		for input in inputs.iter() {
			assert_eq!(parse_unquoted_value::<V>(input).unwrap().1, *input);
		}
	}

	#[test]
	fn test_parse_value() {
		assert_eq!(parse_value::<V>("value1"), Ok(("", "value1".to_string())));
		assert_eq!(
			parse_value::<V>("\"value1\""),
			Ok(("", "value1".to_string()))
		);
		assert_eq!(parse_value::<V>("value 1"), Ok((" 1", "value".to_string())));
		assert_eq!(parse_value::<V>("value\""), Ok(("\"", "value".to_string())));
		assert!(parse_value::<V>("\"value").is_err());
	}

	#[test]
	fn test_parse_multi_line_comment() {
		let input = "/* comment */ rest";
		assert_eq!(
			parse_multi_line_comment::<V>(input).unwrap(),
			(" rest", " comment ")
		);

		let incomplete_input = "/* comment ";
		assert!(parse_multi_line_comment::<V>(incomplete_input).is_err());
	}

	#[test]
	fn test_parse_ws() {
		let input = " \t/* comment */rest";
		assert_eq!(parse_ws::<V>(input).unwrap(), ("rest", " \t/* comment */"));
	}

	#[test]
	fn test_parse_linespace() {
		let input = "\n // comment \n/* multi-line */rest";
		assert_eq!(
			recognize(many0(parse_linespace::<V>))(input).unwrap(),
			("rest", "\n // comment \n/* multi-line */")
		);
	}

	#[test]
	fn test_parse_node_space() {
		let input = " \t/* comment */rest";
		assert_eq!(
			parse_node_space::<V>(input).unwrap(),
			("rest", " \t/* comment */")
		);
	}

	#[test]
	fn test_parse_node_terminator() {
		let input = ";\nrest";
		assert_eq!(parse_node_terminator::<V>(input).unwrap(), ("\nrest", ";"));
		let eof_input = "";
		assert_eq!(parse_node_terminator::<V>(eof_input).unwrap(), ("", ""));
	}

	#[test]
	fn test_parse_kdl_with_error() {
		let input = "node1 key1=value1 { child1 key2=value2; child2 key3=\"value3\" node2";
		let result = parse_kdl(input);
		assert!(result.is_err());
	}

	#[test]
	fn test_kdlnode_get_property() {
		let node = KDLNode {
			name: "node".to_string(),
			properties: vec![
				("key1".to_string(), "value1".to_string()),
				("key2".to_string(), "value2".to_string()),
			],
			children: vec![],
		};
		assert_eq!(node.get_property("key1"), Some(&"value1".to_string()));
		assert_eq!(node.get_property("key2"), Some(&"value2".to_string()));
		assert_eq!(node.get_property("key3"), None);
	}
}
