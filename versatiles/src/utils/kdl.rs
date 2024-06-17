use anyhow::{Context, Result};
use nom::{
	branch::alt,
	bytes::complete::{tag, take_while, take_while1},
	character::complete::{alphanumeric1, char, multispace0, space1},
	combinator::{opt, recognize},
	error::{context, ContextError, ParseError, VerboseError},
	multi::{many0, separated_list0},
	sequence::{delimited, pair},
	IResult,
};

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

// Parse an identifier (Bare Identifier or quoted String)
fn parse_identifier<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	context(
		"identifier",
		alt((parse_quoted_string, parse_bare_identifier)),
	)(input)
}

// Parse a Bare Identifier
fn parse_bare_identifier<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	context(
		"bare identifier",
		recognize(pair(
			take_while1(|c: char| is_initial_identifier_char(c)),
			take_while(|c: char| is_identifier_char(c)),
		)),
	)(input)
}

// Check if a character is valid as the initial character of a Bare Identifier
fn is_initial_identifier_char(c: char) -> bool {
	!c.is_digit(10) && !is_non_identifier_char(c)
}

// Check if a character is valid as any part of a Bare Identifier
fn is_identifier_char(c: char) -> bool {
	!is_non_identifier_char(c)
}

// Check if a character is a non-identifier character
fn is_non_identifier_char(c: char) -> bool {
	c <= '\u{20}' || c == '\u{10FFFF}' || "/(){}<>;[]=,".contains(c)
}

// Parse a quoted string (Quoted Identifier)
fn parse_quoted_string<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	context(
		"quoted string",
		delimited(char('"'), take_while(|c| c != '"'), char('"')),
	)(input)
}

// Parse a key-value pair
fn parse_key_value<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, (String, String), E> {
	let (input, key) = parse_identifier(input)?;
	let (input, _) = char('=')(input)?;
	let (input, value) = parse_value(input)?;

	Ok((input, (key.to_string(), value.to_string())))
}

// Parse a value, either quoted or unquoted
fn parse_value<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, &str, E> {
	alt((parse_quoted_string, parse_unquoted_value))(input)
}

// Parse an unquoted value
fn parse_unquoted_value<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &str, E> {
	recognize(many0(alt((alphanumeric1, tag(".")))))(input)
}

// Parse a KDL node
fn parse_node<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
	input: &'a str,
) -> IResult<&'a str, KDLNode, E> {
	let (input, name) = parse_identifier(input)?;
	let (input, _) = multispace0(input)?;
	let (input, properties) = separated_list0(space1, parse_key_value)(input)?;
	let (input, _) = multispace0(input)?;
	let (input, children) = opt(delimited(char('{'), many0(parse_node), char('}')))(input)?;

	Ok((
		input,
		KDLNode {
			name: name.to_string(),
			properties,
			children: children.unwrap_or_default(),
		},
	))
}

// Parse the entire KDL document
pub fn parse_kdl(input: &str) -> Result<Vec<KDLNode>> {
	match many0(delimited(
		multispace0,
		parse_node::<VerboseError<&str>>,
		multispace0,
	))(input)
	{
		Ok((_, nodes)) => Ok(nodes),
		Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
			let err_msg = nom::error::convert_error(input, e);
			Err(anyhow::anyhow!("Error parsing KDL: {}", err_msg)).context("Failed to parse KDL input")
		}
		Err(e) => {
			Err(anyhow::anyhow!("Error parsing KDL: {:?}", e)).context("Failed to parse KDL input")
		}
	}
}
