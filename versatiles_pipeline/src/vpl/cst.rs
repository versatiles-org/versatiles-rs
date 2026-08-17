//! The VPL concrete syntax tree: every byte of the source, including the parts the engine
//! does not care about.
//!
//! [`VPLPipeline`] is the *semantic* tree — what an operation needs to run. It deliberately
//! forgets comments, whitespace, the order the parameters were written in, and which quotes the
//! author chose. That is the right shape for executing a pipeline and the wrong one for editing
//! a file somebody wrote by hand, because saving would rewrite parts nobody touched.
//!
//! The CST keeps all of it. Every token owns the trivia — whitespace and `#` comments —
//! immediately *before* it, so printing is a concatenation in tree order and
//! `print(parse(x)) == x` holds by construction rather than by care. [`CstPipeline::lower`]
//! converts to the semantic tree when it is time to run something.
//!
//! # Owned text
//!
//! Tokens hold `String`, not borrowed slices. A tree that borrows its source cannot be moved
//! into a caller's own store, cannot cross an FFI boundary, and cannot represent text being
//! inserted that is not already in the file. [`CstToken::span`] keeps the original byte range
//! where there was one, and is `None` for anything built rather than parsed.

use super::{VPLNode, VPLPipeline, serializer::quote};
use std::{
	collections::BTreeMap,
	fmt::{self, Display},
	ops::Range,
};

/// A single piece of source text, together with the trivia that preceded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstToken {
	/// Whitespace and comments between the previous token and this one. Often empty.
	pub leading: String,
	/// The token itself, verbatim — quotes and escapes exactly as written.
	pub text: String,
	/// Byte range of `text` in the source it was parsed from; `None` if synthesized.
	///
	/// The range covers `text` only. The trivia occupies the `leading.len()` bytes before it.
	pub span: Option<Range<usize>>,
}

impl CstToken {
	/// A token with no preceding trivia and no source position.
	#[must_use]
	pub fn new(text: impl Into<String>) -> Self {
		CstToken {
			leading: String::new(),
			text: text.into(),
			span: None,
		}
	}

	/// A token preceded by `leading`, with no source position.
	#[must_use]
	pub fn with_leading(leading: impl Into<String>, text: impl Into<String>) -> Self {
		CstToken {
			leading: leading.into(),
			text: text.into(),
			span: None,
		}
	}
}

impl CstToken {
	/// Replaces the token text, keeping the trivia and dropping the now-stale span.
	pub fn set_text(&mut self, text: impl Into<String>) {
		self.text = text.into();
		self.span = None;
	}
}

impl Display for CstToken {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.leading)?;
		f.write_str(&self.text)
	}
}

/// A list whose elements are separated by a token: nodes by `|`, array items and sources by `,`.
///
/// The separator is stored on the element that *follows* it, so inserting or removing an element
/// is a local edit rather than one that has to keep a parallel vector in step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Punctuated<T> {
	/// Elements in source order. Only the first may have `separator: None`.
	pub items: Vec<PunctuatedItem<T>>,
}

// Derived `Default` would demand `T: Default`, which an empty list plainly does not need.
impl<T> Default for Punctuated<T> {
	fn default() -> Self {
		Punctuated { items: Vec::new() }
	}
}

/// One element of a [`Punctuated`] list, with the separator that introduced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PunctuatedItem<T> {
	/// The separator before this element; `None` for the first element of a list.
	pub separator: Option<CstToken>,
	/// The element itself.
	pub value: T,
}

impl<T> Punctuated<T> {
	/// Builds a list from values, giving every element after the first the separator `separator`.
	///
	/// The separator carries its own trivia, so `Punctuated::new(nodes, CstToken::with_leading(" ", "|"))`
	/// produces `a |b`; pass a token whose `text` already ends in a space for `a | b`.
	#[must_use]
	pub fn new(values: impl IntoIterator<Item = T>, separator: &CstToken) -> Self {
		let items = values
			.into_iter()
			.enumerate()
			.map(|(i, value)| PunctuatedItem {
				separator: (i > 0).then(|| separator.clone()),
				value,
			})
			.collect();
		Punctuated { items }
	}

	/// Iterates over the elements, ignoring separators.
	pub fn iter(&self) -> impl Iterator<Item = &T> {
		self.items.iter().map(|item| &item.value)
	}

	/// Number of elements.
	#[must_use]
	pub fn len(&self) -> usize {
		self.items.len()
	}

	/// Whether the list has no elements.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}

	/// Iterates mutably over the elements, ignoring separators.
	pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
		self.items.iter_mut().map(|item| &mut item.value)
	}

	/// Appends an element, giving it `separator` unless it is the first.
	pub fn push(&mut self, value: T, separator: CstToken) {
		let separator = (!self.items.is_empty()).then_some(separator);
		self.items.push(PunctuatedItem { separator, value });
	}

	/// Removes the element at `index`, returning it.
	///
	/// Removing the first element clears the new first element's separator: a list that begins
	/// with its separator would print a leading `|` or `,` and no longer parse.
	pub fn remove(&mut self, index: usize) -> Option<T> {
		if index >= self.items.len() {
			return None;
		}
		let removed = self.items.remove(index);
		if index == 0
			&& let Some(first) = self.items.first_mut()
		{
			first.separator = None;
		}
		Some(removed.value)
	}
}

impl<T: Display> Display for Punctuated<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for item in &self.items {
			if let Some(separator) = &item.separator {
				write!(f, "{separator}")?;
			}
			write!(f, "{}", item.value)?;
		}
		Ok(())
	}
}

/// A whole VPL source file.
///
/// Exists because trailing trivia has no token to attach to: the grammar allows whitespace and
/// comments after the last node, and something has to hold them for the round trip to be exact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstFile {
	/// The pipeline itself. Leading trivia belongs to its first token.
	pub pipeline: CstPipeline,
	/// Whitespace and comments after the last token.
	pub trailing: String,
}

impl CstFile {
	/// Wraps a pipeline with no trailing trivia.
	#[must_use]
	pub fn new(pipeline: CstPipeline) -> Self {
		CstFile {
			pipeline,
			trailing: String::new(),
		}
	}

	/// Converts to the semantic tree, discarding trivia and formatting.
	#[must_use]
	pub fn lower(&self) -> VPLPipeline {
		self.pipeline.lower()
	}

	/// Recomputes every token's span from the tree's current text.
	///
	/// Editing invalidates spans — a token whose text changed no longer sits where it did, and
	/// neither does anything after it. Because the tree is lossless, walking it in order and
	/// accumulating `leading.len() + text.len()` reproduces the positions the text now has, so
	/// call this after a round of edits to make the spans address the printed result again.
	pub fn reindex_spans(&mut self) {
		let mut offset = 0;
		walk_pipeline(&mut self.pipeline, &mut offset);
	}
}

impl Display for CstFile {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.pipeline)?;
		f.write_str(&self.trailing)
	}
}

/// A sequence of nodes separated by `|`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstPipeline {
	/// The nodes, in source order.
	pub nodes: Punctuated<CstNode>,
}

impl CstPipeline {
	/// Converts to the semantic tree, discarding trivia and formatting.
	#[must_use]
	pub fn lower(&self) -> VPLPipeline {
		VPLPipeline::new(self.nodes.iter().map(CstNode::lower).collect())
	}
}

impl CstPipeline {
	/// Appends a node, separating it from the previous one with ` | `.
	pub fn push_node(&mut self, mut node: CstNode) {
		if !self.nodes.is_empty() && node.name.leading.is_empty() {
			node.name.leading = " ".to_string();
		}
		self.nodes.push(node, CstToken::with_leading(" ", "|"));
	}

	/// Removes the node at `index`, keeping the remaining separators valid.
	pub fn remove_node(&mut self, index: usize) -> Option<CstNode> {
		self.nodes.remove(index)
	}
}

impl Display for CstPipeline {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.nodes)
	}
}

/// One operation: a name, its parameters in source order, and optional child pipelines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstNode {
	/// The operation name.
	pub name: CstToken,
	/// Parameters in the order they were written, duplicates kept apart.
	pub properties: Vec<CstProperty>,
	/// The bracketed child pipelines, if the node has any.
	pub sources: Option<CstSources>,
}

impl CstNode {
	/// A node with no parameters and no sources.
	#[must_use]
	pub fn new(name: impl Into<String>) -> Self {
		CstNode {
			name: CstToken::new(name),
			properties: Vec::new(),
			sources: None,
		}
	}

	/// The first parameter named `key`, if the node has one.
	#[must_use]
	pub fn property(&self, key: &str) -> Option<&CstProperty> {
		self.properties.iter().find(|property| property.key.text == key)
	}

	/// The first parameter named `key`, mutably.
	pub fn property_mut(&mut self, key: &str) -> Option<&mut CstProperty> {
		self.properties.iter_mut().find(|property| property.key.text == key)
	}

	/// Sets `key` to `value`, updating the parameter in place if it is already there and
	/// appending ` key=value` if it is not.
	///
	/// Only the first parameter with that name is updated. The grammar permits repeats — they are
	/// merged into one list when lowering — so use [`remove_property`](Self::remove_property)
	/// first if you mean to replace a repeated key rather than edit its first occurrence.
	pub fn set_property(&mut self, key: &str, value: &str) {
		if let Some(property) = self.property_mut(key) {
			property.set_value(value);
		} else {
			self.properties.push(CstProperty::new(key, value));
		}
	}

	/// Removes every parameter named `key`, returning how many went.
	pub fn remove_property(&mut self, key: &str) -> usize {
		let before = self.properties.len();
		self.properties.retain(|property| property.key.text != key);
		before - self.properties.len()
	}

	/// Converts to the semantic tree.
	///
	/// Repeated keys are appended into one value list and the result is alphabetised, matching
	/// what the parser does when it builds a [`VPLNode`] directly.
	#[must_use]
	pub fn lower(&self) -> VPLNode {
		let mut properties: BTreeMap<String, Vec<String>> = BTreeMap::new();
		for property in &self.properties {
			let mut values = property.value.decode();
			properties
				.entry(property.key.text.clone())
				.and_modify(|list| list.append(&mut values))
				.or_insert(values);
		}

		VPLNode {
			name: self.name.text.clone(),
			properties,
			sources: self
				.sources
				.as_ref()
				.map(|sources| sources.pipelines.iter().map(CstPipeline::lower).collect())
				.unwrap_or_default(),
		}
	}
}

impl Display for CstNode {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.name)?;
		for property in &self.properties {
			write!(f, "{property}")?;
		}
		if let Some(sources) = &self.sources {
			write!(f, "{sources}")?;
		}
		Ok(())
	}
}

/// A single `key=value` parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstProperty {
	/// The parameter name.
	pub key: CstToken,
	/// The `=` between key and value, which may carry trivia on either side.
	pub equals: CstToken,
	/// The value.
	pub value: CstValue,
}

impl CstProperty {
	/// A parameter written as ` key=value`, quoting `value` as loosely as the grammar allows.
	#[must_use]
	pub fn new(key: impl Into<String>, value: &str) -> Self {
		CstProperty {
			key: CstToken::with_leading(" ", key),
			equals: CstToken::new("="),
			value: CstValue::Single(CstString::new(value)),
		}
	}
}

impl CstProperty {
	/// Replaces the value, re-quoting it as loosely as the grammar allows.
	///
	/// The trivia before the value is kept, so ` key = value` stays spaced the way it was.
	pub fn set_value(&mut self, value: &str) {
		match &mut self.value {
			CstValue::Single(string) => string.set(value),
			other => {
				let leading = std::mem::take(&mut other.first_token_mut().leading);
				let mut replacement = CstValue::single(value);
				replacement.first_token_mut().leading = leading;
				*other = replacement;
			}
		}
	}
}

impl Display for CstProperty {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}{}{}", self.key, self.equals, self.value)
	}
}

/// The right-hand side of a parameter: one value, or a bracketed list of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CstValue {
	/// A single value, e.g. `level_min=5`.
	Single(CstString),
	/// A bracketed list, e.g. `layer=["place", "water"]`.
	Array(CstArray),
}

impl CstValue {
	/// A single value, quoted as loosely as the grammar allows.
	#[must_use]
	pub fn single(value: &str) -> Self {
		CstValue::Single(CstString::new(value))
	}

	/// A bracketed list, printed as `[a, b, c]`.
	#[must_use]
	pub fn array<I: IntoIterator<Item = S>, S: AsRef<str>>(values: I) -> Self {
		let items = values
			.into_iter()
			.enumerate()
			.map(|(i, value)| {
				let mut string = CstString::new(value.as_ref());
				if i > 0 {
					string.token.leading = " ".to_string();
				}
				PunctuatedItem {
					separator: (i > 0).then(|| CstToken::new(",")),
					value: string,
				}
			})
			.collect();

		CstValue::Array(CstArray {
			open: CstToken::new("["),
			items: Punctuated { items },
			close: CstToken::new("]"),
		})
	}

	/// The first token of the value, which is where its trivia lives.
	fn first_token_mut(&mut self) -> &mut CstToken {
		match self {
			CstValue::Single(string) => &mut string.token,
			CstValue::Array(array) => &mut array.open,
		}
	}

	/// The decoded values, one entry for a single value and one per item for an array.
	#[must_use]
	pub fn decode(&self) -> Vec<String> {
		match self {
			CstValue::Single(string) => vec![string.decode()],
			CstValue::Array(array) => array.items.iter().map(CstString::decode).collect(),
		}
	}
}

impl Display for CstValue {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			CstValue::Single(string) => write!(f, "{string}"),
			CstValue::Array(array) => write!(f, "{array}"),
		}
	}
}

/// A bracketed list of values, e.g. `["place", "water"]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstArray {
	/// The `[`.
	pub open: CstToken,
	/// The values, comma-separated.
	pub items: Punctuated<CstString>,
	/// The `]`, whose trivia is any space before the bracket.
	pub close: CstToken,
}

impl Display for CstArray {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}{}{}", self.open, self.items, self.close)
	}
}

/// A bracketed list of child pipelines, e.g. `[ a | b, c ]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstSources {
	/// The `[`.
	pub open: CstToken,
	/// The child pipelines, comma-separated.
	pub pipelines: Punctuated<CstPipeline>,
	/// The `]`.
	pub close: CstToken,
}

impl Display for CstSources {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}{}{}", self.open, self.pipelines, self.close)
	}
}

/// How a string was spelled in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CstStringKind {
	/// Unquoted, e.g. `webp`.
	Bare,
	/// Single-quoted, taken literally: `'C:\dir'`.
	Single,
	/// Double-quoted, with `\\`, `\"`, `\n` and `\t` escapes.
	Double,
}

/// A value as written, keeping its quotes and escapes.
///
/// The raw text is the single source of truth; the decoded value is derived on demand by
/// [`decode`](Self::decode) rather than stored, so editing `token.text` cannot leave the two
/// disagreeing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CstString {
	/// The value verbatim, including any quotes.
	pub token: CstToken,
	/// Which spelling `token.text` uses.
	pub kind: CstStringKind,
}

impl CstString {
	/// Writes `value` using the least punctuation that parses back.
	///
	/// Uses the same rules as [`VPLPipeline`]'s `Display`, so the two cannot disagree.
	#[must_use]
	pub fn new(value: &str) -> Self {
		let text = quote(value).into_owned();
		let kind = match text.chars().next() {
			Some('\'') => CstStringKind::Single,
			Some('"') => CstStringKind::Double,
			_ => CstStringKind::Bare,
		};
		CstString {
			token: CstToken::new(text),
			kind,
		}
	}

	/// The value with quotes removed and escapes resolved.
	///
	/// Mirrors the parser's escape rules. An unrecognised escape yields the character after the
	/// backslash; the parser rejects such input, so this only arises in a hand-built tree.
	#[must_use]
	pub fn decode(&self) -> String {
		let text = self.token.text.as_str();
		match self.kind {
			CstStringKind::Bare => text.to_string(),
			CstStringKind::Single => unwrap_quotes(text, '\'').to_string(),
			CstStringKind::Double => {
				let body = unwrap_quotes(text, '"');
				let mut result = String::with_capacity(body.len());
				let mut chars = body.chars();
				while let Some(c) = chars.next() {
					if c != '\\' {
						result.push(c);
						continue;
					}
					match chars.next() {
						Some('n') => result.push('\n'),
						Some('t') => result.push('\t'),
						// `\\` and `\"` stand for themselves; so does anything else, which the
						// parser would not have accepted in the first place.
						Some(other) => result.push(other),
						None => result.push('\\'),
					}
				}
				result
			}
		}
	}
}

impl CstString {
	/// Replaces the value, re-quoting it and keeping the trivia before it.
	pub fn set(&mut self, value: &str) {
		let replacement = CstString::new(value);
		self.token.set_text(replacement.token.text);
		self.kind = replacement.kind;
	}
}

impl Display for CstString {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.token)
	}
}

// ---------------------------------------------------------------------------------------------
// Span assignment
// ---------------------------------------------------------------------------------------------

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

/// Strips a matching pair of `quote` characters, leaving unbalanced text alone.
fn unwrap_quotes(text: &str, quote: char) -> &str {
	text
		.strip_prefix(quote)
		.and_then(|rest| rest.strip_suffix(quote))
		.unwrap_or(text)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vpl::parse_vpl;
	use rstest::rstest;

	fn pipe() -> CstToken {
		CstToken::with_leading(" ", "| ")
	}

	/// `from_container filename='a b' | filter level_min=5`, built by hand.
	fn sample() -> CstFile {
		let mut from = CstNode::new("from_container");
		from.properties.push(CstProperty::new("filename", "a b"));

		let mut filter = CstNode::new("filter");
		filter.properties.push(CstProperty::new("level_min", "5"));

		CstFile::new(CstPipeline {
			nodes: Punctuated::new([from, filter], &pipe()),
		})
	}

	#[test]
	fn hand_built_tree_prints_valid_vpl() {
		assert_eq!(
			sample().to_string(),
			"from_container filename='a b' | filter level_min=5"
		);
	}

	/// The printed text must mean what the tree says it means — checked against the existing
	/// parser, so lowering cannot quietly disagree with it.
	#[test]
	fn lowering_agrees_with_parsing_the_printed_text() {
		let file = sample();
		assert_eq!(file.lower(), parse_vpl(&file.to_string()).unwrap());
	}

	/// Trivia is the whole point: comments and layout have to survive a print unchanged.
	#[test]
	fn trivia_is_preserved_verbatim() {
		let mut node = CstNode::new("from_container");
		node.name.leading = "# a leading comment\n".to_string();
		node.properties.push(CstProperty {
			key: CstToken::with_leading("\n\t", "filename"),
			equals: CstToken::new(" = "),
			value: CstValue::Single(CstString::new("berlin.versatiles")),
		});

		let file = CstFile {
			pipeline: CstPipeline {
				nodes: Punctuated::new([node], &pipe()),
			},
			trailing: " # trailing\n".to_string(),
		};

		assert_eq!(
			file.to_string(),
			"# a leading comment\nfrom_container\n\tfilename = berlin.versatiles # trailing\n"
		);
		// and the meaning survives too
		assert_eq!(file.lower(), parse_vpl(&file.to_string()).unwrap());
	}

	#[rstest]
	#[case("value", "value")]
	#[case("", "''")]
	#[case("a b", "'a b'")]
	#[case("it's", r#""it's""#)]
	#[case("line\nbreak", r#""line\nbreak""#)]
	fn new_string_quotes_like_the_serialiser(#[case] value: &str, #[case] expected_text: &str) {
		let string = CstString::new(value);
		assert_eq!(string.token.text, expected_text);
		// whatever spelling it picked, decoding returns the original value
		assert_eq!(string.decode(), value);
	}

	#[rstest]
	#[case(CstStringKind::Bare, "webp", "webp")]
	#[case(CstStringKind::Single, "'a b'", "a b")]
	#[case(CstStringKind::Single, r"'C:\dir'", r"C:\dir")]
	#[case(CstStringKind::Double, r#""a\nb""#, "a\nb")]
	#[case(CstStringKind::Double, r#""a\tb""#, "a\tb")]
	#[case(CstStringKind::Double, r#""a\\b""#, r"a\b")]
	#[case(CstStringKind::Double, r#""a\"b""#, "a\"b")]
	#[case(CstStringKind::Double, r#""""#, "")]
	#[case(CstStringKind::Single, "''", "")]
	// not something the parser accepts; decoding must still be total
	#[case(CstStringKind::Double, r#""a\qb""#, "aqb")]
	fn decode_mirrors_the_parser(#[case] kind: CstStringKind, #[case] text: &str, #[case] expected: &str) {
		let string = CstString {
			token: CstToken::new(text),
			kind,
		};
		assert_eq!(string.decode(), expected);
	}

	/// Duplicate keys merge into one list, exactly as the parser does when building a `VPLNode`.
	#[test]
	fn lowering_merges_repeated_keys() {
		let mut node = CstNode::new("node");
		node.properties.push(CstProperty::new("key", "a"));
		node.properties.push(CstProperty::new("key", "b"));

		let file = CstFile::new(CstPipeline {
			nodes: Punctuated::new([node], &pipe()),
		});

		assert_eq!(file.to_string(), "node key=a key=b");
		assert_eq!(file.lower(), parse_vpl("node key=a key=b").unwrap());
	}

	#[test]
	fn arrays_and_sources_round_trip() {
		let mut child = CstNode::new("child");
		child.properties.push(CstProperty::new("layer", "a b"));

		let mut parent = CstNode::new("from_stacked");
		parent.properties.push(CstProperty {
			key: CstToken::with_leading(" ", "layer"),
			equals: CstToken::new("="),
			value: CstValue::Array(CstArray {
				open: CstToken::new("["),
				items: Punctuated::new([CstString::new("place"), CstString::new("water")], &CstToken::new(", ")),
				close: CstToken::new("]"),
			}),
		});
		parent.sources = Some(CstSources {
			open: CstToken::with_leading(" ", "[ "),
			pipelines: Punctuated::new(
				[CstPipeline {
					nodes: Punctuated::new([child], &pipe()),
				}],
				&CstToken::new(", "),
			),
			close: CstToken::with_leading(" ", "]"),
		});

		let file = CstFile::new(CstPipeline {
			nodes: Punctuated::new([parent], &pipe()),
		});

		assert_eq!(
			file.to_string(),
			"from_stacked layer=[place, water] [ child layer='a b' ]"
		);
		assert_eq!(file.lower(), parse_vpl(&file.to_string()).unwrap());
	}

	#[test]
	fn empty_bracket_lists_print_as_written() {
		let mut node = CstNode::new("node");
		node.properties.push(CstProperty {
			key: CstToken::with_leading(" ", "key"),
			equals: CstToken::new("="),
			value: CstValue::Array(CstArray {
				open: CstToken::new("["),
				items: Punctuated::default(),
				close: CstToken::new("]"),
			}),
		});

		let file = CstFile::new(CstPipeline {
			nodes: Punctuated::new([node], &pipe()),
		});

		assert_eq!(file.to_string(), "node key=[]");
		assert_eq!(file.lower(), parse_vpl("node key=[]").unwrap());
	}

	// -----------------------------------------------------------------------------------------
	// Editing
	// -----------------------------------------------------------------------------------------

	use crate::vpl::parse_cst;

	/// Edits are only useful if what comes out still parses, so every case below prints the edited
	/// tree and parses it again rather than trusting the string.
	fn edited(source: &str, edit: impl FnOnce(&mut CstFile)) -> String {
		let mut file = parse_cst(source).unwrap();
		edit(&mut file);
		let printed = file.to_string();
		parse_cst(&printed).unwrap_or_else(|e| panic!("edit produced unparseable VPL {printed:?}:\n{e}"));
		printed
	}

	/// The point of the whole exercise: changing one value leaves everything else byte-identical.
	#[test]
	fn setting_a_value_leaves_the_rest_of_the_file_alone() {
		let source = "# which file\nfrom_container  filename = 'berlin.versatiles'  # note\n| filter level_min=5";
		let printed = edited(source, |file| {
			file.pipeline.nodes.items[0]
				.value
				.set_property("filename", "hamburg.versatiles");
		});

		assert_eq!(
			printed,
			"# which file\nfrom_container  filename = hamburg.versatiles  # note\n| filter level_min=5"
		);
	}

	/// A value that needs quoting gets it, without the caller having to know the rules.
	#[test]
	fn setting_a_value_quotes_it_as_needed() {
		let printed = edited("node key=plain", |file| {
			file.pipeline.nodes.items[0].value.set_property("key", "needs quoting");
		});
		assert_eq!(printed, "node key='needs quoting'");

		let printed = edited("node key=plain", |file| {
			file.pipeline.nodes.items[0].value.set_property("key", "it's awkward");
		});
		assert_eq!(printed, r#"node key="it's awkward""#);
	}

	#[test]
	fn adding_a_parameter_places_it_before_the_sources() {
		let printed = edited("from_stacked [ a, b ]", |file| {
			file.pipeline.nodes.items[0].value.set_property("level_min", "5");
		});
		assert_eq!(printed, "from_stacked level_min=5 [ a, b ]");
	}

	#[test]
	fn removing_a_parameter_takes_its_whitespace_with_it() {
		let printed = edited("node alpha=1 beta=2 gamma=3", |file| {
			assert_eq!(file.pipeline.nodes.items[0].value.remove_property("beta"), 1);
		});
		assert_eq!(printed, "node alpha=1 gamma=3");
	}

	/// Repeated keys are legal, so removing by name has to take all of them.
	#[test]
	fn removing_a_repeated_parameter_takes_every_copy() {
		let printed = edited("node key=a key=b other=c", |file| {
			assert_eq!(file.pipeline.nodes.items[0].value.remove_property("key"), 2);
		});
		assert_eq!(printed, "node other=c");
	}

	#[test]
	fn pushing_a_node_separates_it_with_a_pipe() {
		let printed = edited("from_container filename=a", |file| {
			let mut node = CstNode::new("filter");
			node.set_property("level_min", "5");
			file.pipeline.push_node(node);
		});
		assert_eq!(printed, "from_container filename=a | filter level_min=5");
	}

	/// The invariant worth protecting: a list that kept its separator after losing its first
	/// element would print a leading `|` and stop parsing.
	#[test]
	fn removing_the_first_node_does_not_leave_a_dangling_separator() {
		let printed = edited("alpha | beta | gamma", |file| {
			assert_eq!(file.pipeline.remove_node(0).unwrap().name.text, "alpha");
		});
		assert!(!printed.trim_start().starts_with('|'), "got {printed:?}");
		assert_eq!(parse_cst(&printed).unwrap().lower().len(), 2);
	}

	#[test]
	fn arrays_can_be_built_without_knowing_the_syntax() {
		let printed = edited("node key=old", |file| {
			file.pipeline.nodes.items[0].value.properties[0].value = CstValue::array(["place", "water park", "it's"]);
		});
		assert_eq!(printed, r#"node key=[place, 'water park', "it's"]"#);
		assert_eq!(
			parse_cst(&printed).unwrap().pipeline.nodes.items[0].value.properties[0]
				.value
				.decode(),
			["place", "water park", "it's"]
		);
	}

	/// Replacing an array with a single value must keep the spacing around it.
	#[test]
	fn replacing_an_array_with_a_single_value_keeps_the_spacing() {
		let printed = edited("node key = [a, b]", |file| {
			file.pipeline.nodes.items[0].value.set_property("key", "one");
		});
		assert_eq!(printed, "node key = one");
	}

	/// Spans are stale the moment the text changes, and correct again after a reindex.
	#[test]
	fn reindexing_makes_spans_address_the_edited_text() {
		let mut file = parse_cst("node key=short | other").unwrap();
		file.pipeline.nodes.items[0]
			.value
			.set_property("key", "considerably-longer");

		let printed = file.to_string();
		file.reindex_spans();

		let value = &file.pipeline.nodes.items[0].value.properties[0].value;
		let CstValue::Single(string) = value else {
			panic!("expected a single value")
		};
		assert_eq!(&printed[string.token.span.clone().unwrap()], "considerably-longer");
		// tokens after the edit moved too
		let second = &file.pipeline.nodes.items[1].value;
		assert_eq!(&printed[second.name.span.clone().unwrap()], "other");
	}
}
