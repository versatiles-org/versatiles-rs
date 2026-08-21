//! Reformatting a [`CstFile`] without discarding what the author wrote.
//!
//! [`VPLPipeline::to_string_pretty`](crate::vpl::VPLPipeline::to_string_pretty) formats the
//! *semantic* tree, which has already forgotten the comments — so "reformat this file" and "keep
//! what I wrote" are otherwise exclusive, and an editor offering a Format command has to either
//! delete the user's comments or grow a second formatter that disagrees with the first.
//!
//! This is the same layout applied to the tree that still has them. Every token owns the trivia
//! immediately before it, so the whole job is rewriting those strings: no node is moved, no
//! parameter reordered, no value re-quoted, and `print(parse(x)) == x` stays true of everything
//! that is not whitespace.

use std::fmt::Write as _;

use super::cst::{CstFile, CstNode, CstPipeline, CstToken, CstValue};

/// One tab per level, matching
/// [`to_string_pretty`](crate::vpl::VPLPipeline::to_string_pretty).
fn indent(level: usize) -> String {
	"\t".repeat(level)
}

/// A line break followed by `level` tabs.
fn line(level: usize) -> String {
	format!("\n{}", indent(level))
}

/// The `#` comments in one run of trivia, in source order, each without its
/// trailing whitespace.
///
/// Trivia is whitespace and comments and nothing else — the parser's `ws0`
/// admits nothing more — so a `#` always opens a comment that runs to the end
/// of its line.
fn comments_in(trivia: &str) -> Vec<&str> {
	trivia
		.lines()
		.filter_map(|line| line.find('#').map(|start| line[start..].trim_end()))
		.collect()
}

/// Rewrites `token.leading` to `natural`, keeping the comments that were in it.
///
/// The comments go on their own lines at `level`, which is the indentation of
/// the line the token itself sits on. A comment runs to the end of its line, so
/// whatever follows one has to start a new line — which is why a token that
/// carries comments gets a line break instead of `natural`, even where the
/// layout would otherwise have put it on the line above.
fn retrivia(token: &mut CstToken, natural: &str, level: usize) {
	let comments = comments_in(&token.leading);
	if comments.is_empty() {
		token.leading.clear();
		token.leading.push_str(natural);
		return;
	}

	let line = line(level);
	let mut leading = String::new();
	for comment in comments {
		write!(leading, "{line}{comment}").expect("writing to a String");
	}
	leading.push_str(&line);
	token.leading = leading;
}

/// Lays out one pipeline at `level`, with `head` as the trivia before its first
/// operation.
///
/// The `|` leads its line at the pipeline's own level and the operation follows
/// it after a single space, so the operations read as one column however deeply
/// their parameters nest.
fn format_pipeline(pipeline: &mut CstPipeline, level: usize, head: &str) {
	let separator_line = line(level);
	for (index, item) in pipeline.nodes.items.iter_mut().enumerate() {
		if let Some(separator) = &mut item.separator {
			retrivia(separator, &separator_line, level);
		}
		format_node(&mut item.value, level, if index == 0 { head } else { " " });
	}
}

/// Lays out one operation whose name sits at `level`, with its parameters and
/// sources one level in.
fn format_node(node: &mut CstNode, level: usize, natural: &str) {
	retrivia(&mut node.name, natural, level);

	let property_line = line(level + 1);
	for property in &mut node.properties {
		retrivia(&mut property.key, &property_line, level + 1);
		retrivia(&mut property.equals, "", level + 1);
		format_value(&mut property.value, level + 1);
	}

	if let Some(sources) = &mut node.sources {
		retrivia(&mut sources.open, " ", level);
		let source_head = line(level + 1);
		for item in &mut sources.pipelines.items {
			if let Some(separator) = &mut item.separator {
				// The comma belongs to the source it follows, on the line that source ends on.
				retrivia(separator, "", level + 1);
			}
			format_pipeline(&mut item.value, level + 1, &source_head);
		}
		// The grammar has no trailing comma, so the closing bracket gets its own
		// line at the operation's level.
		retrivia(&mut sources.close, &line(level), level);
	}
}

/// Lays out one parameter value, whose line is the parameter's own.
fn format_value(value: &mut CstValue, level: usize) {
	match value {
		CstValue::Single(string) => retrivia(&mut string.token, "", level),
		CstValue::Array(array) => {
			retrivia(&mut array.open, "", level);
			for (index, item) in array.items.items.iter_mut().enumerate() {
				if let Some(separator) = &mut item.separator {
					retrivia(separator, "", level);
				}
				retrivia(&mut item.value.token, if index == 0 { "" } else { " " }, level);
			}
			retrivia(&mut array.close, "", level);
		}
	}
}

/// Lays out the trivia after the last token: one comment per line, then the
/// final newline a file ends with.
fn format_trailing(trailing: &str) -> String {
	let mut out = String::new();
	for comment in comments_in(trailing) {
		write!(out, "\n{comment}").expect("writing to a String");
	}
	out.push('\n');
	out
}

impl CstFile {
	/// Rewrites every token's leading trivia to `to_string_pretty`'s layout,
	/// keeping the comments in it.
	///
	/// One tab per level, `|` at the pipeline's own level, parameters and nested
	/// sources one level in — the same rules as
	/// [`to_string_pretty`](crate::vpl::VPLPipeline::to_string_pretty), because
	/// two formatters that disagree is the failure worth avoiding here. The
	/// file ends with a newline.
	///
	/// **Whitespace and nothing else.** Parameters keep the order they were
	/// written in, values keep the quotes the author chose, and a one-element
	/// list stays a list — all of which `to_string_pretty` normalises, and all
	/// of which would be rewriting parts nobody touched.
	///
	/// Every comment keeps the token it was attached to and takes that token's
	/// line and indentation: a comment before a parameter ends up on its own
	/// line at the parameter's indent, one before an operation at the
	/// operation's. Since a comment runs to the end of its line, a token that
	/// carries one always starts a new line, even where the layout would have
	/// kept it on the line above.
	///
	/// Idempotent, and spans are reindexed, so an editor's positions address the
	/// formatted text when this returns.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_pipeline::vpl::parse_cst;
	///
	/// let mut file = parse_cst("# the debug source\nfrom_debug format=png | filter level_min=2")?;
	/// file.format();
	/// assert_eq!(
	///     file.to_string(),
	///     "# the debug source\nfrom_debug\n\tformat=png\n| filter\n\tlevel_min=2\n"
	/// );
	/// # Ok::<(), versatiles_pipeline::vpl::VplParseError>(())
	/// ```
	pub fn format(&mut self) {
		format_pipeline(&mut self.pipeline, 0, "");

		// The first token has no line above it, so a comment attached to it must
		// not open the file with a blank one.
		if let Some(first) = self.pipeline.nodes.items.first_mut()
			&& let Some(rest) = first.value.name.leading.strip_prefix('\n')
		{
			first.value.name.leading = rest.to_string();
		}

		self.trailing = format_trailing(&self.trailing);
		self.reindex_spans();
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use crate::vpl::{parse_cst, parse_vpl};

	/// `format` applied to `source`, as text.
	fn formatted(source: &str) -> String {
		let mut file = parse_cst(source).expect("valid VPL");
		file.format();
		file.to_string()
	}

	#[test]
	fn comments_survive_and_take_their_token_s_line() {
		assert_eq!(
			formatted("# the debug source, until the real one lands\nfrom_debug format=png | filter level_min=2"),
			"# the debug source, until the real one lands\nfrom_debug\n\tformat=png\n| filter\n\tlevel_min=2\n"
		);
		// A comment before a parameter lands at the parameter's indent, not the
		// operation's.
		assert_eq!(
			formatted("from_debug # png until webp lands\nformat=png"),
			"from_debug\n\t# png until webp lands\n\tformat=png\n"
		);
		// Several comments on one token keep their order, one per line.
		assert_eq!(
			formatted("# first\n# second\nfrom_debug format=png"),
			"# first\n# second\nfrom_debug\n\tformat=png\n"
		);
	}

	#[test]
	fn trailing_comments_keep_their_own_lines() {
		assert_eq!(
			formatted("from_debug format=png # why png\n"),
			"from_debug\n\tformat=png\n# why png\n"
		);
		// A file with nothing after the last token still ends with a newline.
		assert_eq!(formatted("from_debug"), "from_debug\n");
	}

	/// The layout is `to_string_pretty`'s, and the test says so directly: two
	/// formatters that disagree is the failure this feature is meant to avoid.
	///
	/// The inputs are comment-free and already written the way the serialiser
	/// spells things — parameters alphabetised, values quoted as loosely as the
	/// grammar allows — because those are the parts `to_string_pretty`
	/// normalises and `format` deliberately does not.
	#[rstest]
	#[case("from_debug format=png")]
	#[case("from_debug format=png | filter level_min=2")]
	#[case("from_debug format=png | filter bbox=[0, 0, 10, 10] level_min=2")]
	#[case("from_stacked [ from_debug format=png, from_debug format=webp ]")]
	#[case("from_stacked [ from_stacked [ from_debug format=png ] | filter level_min=2 ]")]
	fn layout_agrees_with_to_string_pretty(#[case] source: &str) {
		let pretty = parse_vpl(source).expect("valid VPL").to_string_pretty();
		assert_eq!(formatted(source), format!("{pretty}\n"));
	}

	#[rstest]
	#[case("from_debug format=png")]
	#[case("# lead\nfrom_debug   format = 'png'   |  filter level_min=2 # why\n\n")]
	#[case("from_stacked [ from_debug format=png, # second\nfrom_debug format=webp ] # tail")]
	#[case("from_debug\n\t\t\tformat=[png,webp]")]
	fn formatting_twice_is_formatting_once(#[case] source: &str) {
		let once = formatted(source);
		assert_eq!(formatted(&once), once);
	}

	/// Layout changes and meaning does not — the property that makes this safe
	/// to run on a file somebody is editing.
	#[rstest]
	#[case("from_debug format=png")]
	#[case("# lead\nfrom_debug   format = 'png'   |  filter level_min=2 # why\n")]
	#[case("from_stacked [ from_debug format=png, from_debug format=webp ] | filter level_min=2")]
	#[case("from_debug format=[png, webp] zebra=1 alpha=2")]
	fn meaning_is_unchanged(#[case] source: &str) {
		let before = parse_cst(source).expect("valid VPL").lower();
		let after = parse_vpl(&formatted(source)).expect("formatted VPL still parses");
		assert_eq!(format!("{after:?}"), format!("{before:?}"));
	}

	#[test]
	fn nothing_but_whitespace_is_rewritten() {
		// Parameter order, the author's quotes and the bracket form all survive,
		// though `to_string_pretty` would normalise every one of them.
		assert_eq!(
			formatted("from_debug zebra='1' alpha=[2]"),
			"from_debug\n\tzebra='1'\n\talpha=[2]\n"
		);
	}

	#[test]
	fn spans_address_the_formatted_text() {
		let mut file = parse_cst("# lead\nfrom_debug   format=png").expect("valid VPL");
		file.format();
		let text = file.to_string();
		let node = &file.pipeline.nodes.items[0].value;

		let name = node.name.span.clone().expect("name span");
		assert_eq!(&text[name], "from_debug");
		let key = node.properties[0].key.span.clone().expect("key span");
		assert_eq!(&text[key], "format");
	}

	#[test]
	fn a_comment_between_the_pipe_and_the_operation_still_parses() {
		// It strands the `|` on its own line, which is the price of every
		// comment keeping the token it was attached to.
		let text = formatted("from_debug format=png | # why filter\nfilter level_min=2");
		assert_eq!(
			text,
			"from_debug\n\tformat=png\n|\n# why filter\nfilter\n\tlevel_min=2\n"
		);
		assert_eq!(parse_vpl(&text).expect("still parses").len(), 2);
	}
}
