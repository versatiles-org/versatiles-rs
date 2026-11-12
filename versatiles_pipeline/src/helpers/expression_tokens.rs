//! The tokens of a feature expression, for highlighting it as it is typed.
//!
//! A lexer pass only: it never fails, never parses, and runs on half-written
//! text as well as on finished expressions, so an editor can call it on every
//! keystroke. It follows the same rules as the rest of the expression code:
//! strings in every form CEL has, `//` comments, and `$` names.

/// What a token is, for an editor to colour it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExpressionTokenKind {
	/// A `$` name, such as `$zoom`, whether or not it is one an expression can read.
	Special,
	/// A feature property, or a field after a `.`, such as `name` in `$props.name`.
	Property,
	/// A name followed by `(`: a function or a method, such as `has` or `startsWith`.
	Function,
	/// An operator, including `in` and the `?` and `:` of a conditional.
	Operator,
	/// A string literal, quotes included. One that is never closed runs to the end.
	String,
	/// A number, such as `12`, `1.5`, `1e3`, `0x1F` or `3u`.
	Number,
	/// `true` or `false`.
	Bool,
	/// `null`.
	Null,
	/// `(`, `)`, `[`, `]`, `{` or `}`.
	Bracket,
	/// `,` or `.`.
	Punctuation,
	/// A `//` comment, up to the end of its line.
	Comment,
	/// A character no expression can contain, such as `@`.
	Unknown,
}

impl ExpressionTokenKind {
	/// The kind as a lowercase word, such as `special` or `operator`, for a
	/// consumer that passes it on as text.
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			ExpressionTokenKind::Special => "special",
			ExpressionTokenKind::Property => "property",
			ExpressionTokenKind::Function => "function",
			ExpressionTokenKind::Operator => "operator",
			ExpressionTokenKind::String => "string",
			ExpressionTokenKind::Number => "number",
			ExpressionTokenKind::Bool => "bool",
			ExpressionTokenKind::Null => "null",
			ExpressionTokenKind::Bracket => "bracket",
			ExpressionTokenKind::Punctuation => "punctuation",
			ExpressionTokenKind::Comment => "comment",
			ExpressionTokenKind::Unknown => "unknown",
		}
	}
}

/// One token of an expression: its kind and where it lies.
///
/// `start` and `end` count characters (Unicode scalar values) of the text as
/// written, `end` exclusive, so a `$` name counts with its `$`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExpressionToken {
	/// What the token is.
	pub kind: ExpressionTokenKind,
	/// The character offset where it starts.
	pub start: usize,
	/// The character offset just past it.
	pub end: usize,
}

/// A token, and for a string whether it is closed.
pub(crate) struct Lexed {
	pub token: ExpressionToken,
	/// For a [`ExpressionTokenKind::String`]: whether it ends before the text does.
	pub closed: bool,
}

/// Every token of `text`, white space left out.
#[must_use]
pub(crate) fn lex(text: &str) -> Vec<Lexed> {
	let chars: Vec<char> = text.chars().collect();
	let mut out: Vec<Lexed> = Vec::new();
	let mut i = 0;

	while i < chars.len() {
		let start = i;
		let c = chars[i];
		let mut closed = true;

		let kind = if c.is_whitespace() {
			i += 1;
			continue;
		} else if c == '/' && chars.get(i + 1) == Some(&'/') {
			while i < chars.len() && chars[i] != '\n' {
				i += 1;
			}
			ExpressionTokenKind::Comment
		} else if c == '\'' || c == '"' {
			let end = string_end(&chars, i, false);
			closed = end.is_some();
			i = end.unwrap_or(chars.len());
			ExpressionTokenKind::String
		} else if c == '$' {
			i += 1;
			while i < chars.len() && is_identifier_char(chars[i]) {
				i += 1;
			}
			ExpressionTokenKind::Special
		} else if c.is_ascii_digit() {
			i = number_end(&chars, i);
			ExpressionTokenKind::Number
		} else if is_identifier_start(c) {
			while i < chars.len() && is_identifier_char(chars[i]) {
				i += 1;
			}
			let word: String = chars[start..i].iter().collect();
			if matches!(chars.get(i), Some('\'' | '"')) && is_string_prefix(&word) {
				let end = string_end(&chars, i, word.contains(['r', 'R']));
				closed = end.is_some();
				i = end.unwrap_or(chars.len());
				ExpressionTokenKind::String
			} else {
				match word.as_str() {
					"true" | "false" => ExpressionTokenKind::Bool,
					"null" => ExpressionTokenKind::Null,
					"in" => ExpressionTokenKind::Operator,
					_ if next_non_space(&chars, i) == Some('(') => ExpressionTokenKind::Function,
					_ => ExpressionTokenKind::Property,
				}
			}
		} else if let Some(op) = operator_at(&chars, i) {
			i += op.chars().count();
			ExpressionTokenKind::Operator
		} else {
			i += 1;
			match c {
				'(' | ')' | '[' | ']' | '{' | '}' => ExpressionTokenKind::Bracket,
				',' | '.' => ExpressionTokenKind::Punctuation,
				_ => ExpressionTokenKind::Unknown,
			}
		};

		out.push(Lexed {
			token: ExpressionToken { kind, start, end: i },
			closed,
		});
	}
	out
}

/// The operators of CEL, longest first so that `<=` is not read as `<`.
const OPERATORS: [&str; 17] = [
	"==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/", "%", "!", "?", ":", "=",
];

/// The operator that starts at `i`, if one does.
fn operator_at(chars: &[char], i: usize) -> Option<&'static str> {
	OPERATORS
		.into_iter()
		.find(|op| op.chars().enumerate().all(|(k, o)| chars.get(i + k) == Some(&o)))
}

/// The end of the number starting at `start`: digits with their suffix or
/// exponent, such as `1000u`, `1e5` or `0x1F`, and a fraction, such as `1.5`.
fn number_end(chars: &[char], start: usize) -> usize {
	let mut i = start;
	while i < chars.len() && is_identifier_char(chars[i]) {
		i += 1;
	}
	if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
		i += 1;
		while i < chars.len() && is_identifier_char(chars[i]) {
			i += 1;
		}
	}
	// An exponent with a sign, such as `1e-5`.
	if matches!(chars.get(i), Some('+' | '-'))
		&& matches!(chars.get(i - 1), Some('e' | 'E'))
		&& chars.get(i + 1).is_some_and(char::is_ascii_digit)
	{
		i += 1;
		while i < chars.len() && chars[i].is_ascii_digit() {
			i += 1;
		}
	}
	i
}

fn next_non_space(chars: &[char], from: usize) -> Option<char> {
	chars[from..].iter().copied().find(|c| !c.is_whitespace())
}

/// The index just past the string literal whose opening quote is at `start`,
/// or `None` if it is not closed.
///
/// Handles both quotes, the tripled forms that may span lines, and `raw`
/// strings, whose backslashes escape nothing.
pub(crate) fn string_end(chars: &[char], start: usize, raw: bool) -> Option<usize> {
	let quote = chars[start];
	let triple = chars.get(start + 1) == Some(&quote) && chars.get(start + 2) == Some(&quote);
	let closes = |i: usize| {
		chars[i] == quote && (!triple || (chars.get(i + 1) == Some(&quote) && chars.get(i + 2) == Some(&quote)))
	};

	let mut i = start + if triple { 3 } else { 1 };
	while i < chars.len() {
		if !raw && chars[i] == '\\' {
			i += 2;
		} else if closes(i) {
			return Some(i + if triple { 3 } else { 1 });
		} else {
			i += 1;
		}
	}
	None
}

/// Whether `word`, directly before a quote, makes it a raw or bytes literal.
pub(crate) fn is_string_prefix(word: &str) -> bool {
	matches!(
		word,
		"r" | "R" | "b" | "B" | "br" | "bR" | "Br" | "BR" | "rb" | "rB" | "Rb" | "RB"
	)
}

pub(crate) fn is_identifier_start(c: char) -> bool {
	c.is_ascii_alphabetic() || c == '_'
}

pub(crate) fn is_identifier_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Each token as `kind:text`, for comparing whole expressions at a glance.
	fn tokens(text: &str) -> Vec<String> {
		let chars: Vec<char> = text.chars().collect();
		lex(text)
			.into_iter()
			.map(|lexed| {
				let token = lexed.token;
				let text: String = chars[token.start..token.end].iter().collect();
				format!("{}:{text}", token.kind.as_str())
			})
			.collect()
	}

	#[test]
	fn an_expression_is_split_into_its_tokens() {
		assert_eq!(
			tokens("$zoom >= 12 && has($props.name) || kind in ['a', \"b\"]"),
			[
				"special:$zoom",
				"operator:>=",
				"number:12",
				"operator:&&",
				"function:has",
				"bracket:(",
				"special:$props",
				"punctuation:.",
				"property:name",
				"bracket:)",
				"operator:||",
				"property:kind",
				"operator:in",
				"bracket:[",
				"string:'a'",
				"punctuation:,",
				"string:\"b\"",
				"bracket:]",
			]
		);
	}

	#[test]
	fn literals_methods_and_comments() {
		assert_eq!(
			tokens("name.startsWith('A') ? true : null // done\n-1.5e-3 + 0x1F + 3u"),
			[
				"property:name",
				"punctuation:.",
				"function:startsWith",
				"bracket:(",
				"string:'A'",
				"bracket:)",
				"operator:?",
				"bool:true",
				"operator::",
				"null:null",
				"comment:// done",
				"operator:-",
				"number:1.5e-3",
				"operator:+",
				"number:0x1F",
				"operator:+",
				"number:3u",
			]
		);
	}

	#[test]
	fn strings_in_every_form_and_unfinished_text() {
		assert_eq!(
			tokens("r'a\\b' == '''x 'y' z''' && b\"q\""),
			[
				"string:r'a\\b'",
				"operator:==",
				"string:'''x 'y' z'''",
				"operator:&&",
				"string:b\"q\""
			]
		);
		let lexed = lex("kind == 'abc");
		assert!(!lexed.last().unwrap().closed, "an unclosed string runs to the end");
		assert_eq!(tokens("kind == 'abc"), ["property:kind", "operator:==", "string:'abc"]);
		assert_eq!(tokens("$ @ $foo"), ["special:$", "unknown:@", "special:$foo"]);
	}

	/// Offsets count characters, so text before a token may be any script.
	#[test]
	fn offsets_count_characters_not_bytes() {
		let lexed = lex("name == 'Köln' && $zoom");
		let last = &lexed.last().unwrap().token;
		assert_eq!((last.start, last.end), (18, 23));
	}
}
