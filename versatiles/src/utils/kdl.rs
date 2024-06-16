use anyhow::{Context, Result};
use nom::{
	branch::alt,
	bytes::complete::{tag, take_while},
	character::complete::{alphanumeric1, char, multispace0, space1},
	combinator::{opt, recognize},
	multi::{many0, separated_list0},
	sequence::delimited,
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

// Parse an identifier (alphanumeric characters or '-')
fn parse_identifier(input: &str) -> IResult<&str, &str> {
	take_while(|c: char| c.is_alphanumeric() || c == '-')(input)
}

// Parse a quoted value
fn parse_quoted_value(input: &str) -> IResult<&str, &str> {
	delimited(char('"'), take_while(|c| c != '"'), char('"'))(input)
}

// Parse an unquoted value
fn parse_unquoted_value(input: &str) -> IResult<&str, &str> {
	recognize(many0(alt((alphanumeric1, tag(".")))))(input)
}

// Parse a value, either quoted or unquoted
fn parse_value(input: &str) -> IResult<&str, &str> {
	alt((parse_quoted_value, parse_unquoted_value))(input)
}

// Parse a key-value pair
fn parse_key_value(input: &str) -> IResult<&str, (String, String)> {
	let (input, key) = parse_identifier(input)?;
	let (input, _) = char('=')(input)?;
	let (input, value) = parse_value(input)?;

	Ok((input, (key.to_string(), value.to_string())))
}

// Parse a KDL node
fn parse_node(input: &str) -> IResult<&str, KDLNode> {
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
	match many0(delimited(multispace0, parse_node, multispace0))(input) {
		Ok((_, nodes)) => Ok(nodes),
		Err(e) => {
			Err(anyhow::anyhow!("Error parsing KDL: {:?}", e)).context("Failed to parse KDL input")
		}
	}
}
