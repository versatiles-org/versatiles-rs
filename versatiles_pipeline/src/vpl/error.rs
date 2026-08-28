//! Structured VPL parse errors.
//!
//! Nothing here interprets the grammar: the offsets and labels are `nom`'s own, extracted rather
//! than invented. The extraction is possible because every entry in a `VerboseError` holds the
//! input still remaining at that point, which is a suffix of the whole input — so the difference
//! in length is the byte offset. `nom_language`'s `convert_error` computes exactly this and then
//! discards it in favour of a rendered string; [`render_trace`] is a port of that rendering which
//! keeps the offsets and fixes its caret placement.

use std::{fmt, ops::Range};

use nom::{Offset, error::ErrorKind};
use nom_language::error::{VerboseError, VerboseErrorKind};

/// One frame of the parser's context stack.
///
/// Each frame names a construct the parser was in the middle of when it failed, and where that
/// construct started. A consumer that wants to underline the whole construct rather than the
/// single offending character can widen [`VplParseError::span`] to reach back to `offset`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VplErrorFrame {
	/// Name of the construct, e.g. `"parsing property"`.
	pub label: String,
	/// Byte offset into the input at which the construct starts.
	pub offset: usize,
}

/// A VPL parse failure with a machine-readable position.
///
/// [`parse_vpl`](super::parse_vpl) reports failures as a caret-annotated trace, which reads well
/// in a terminal but states the position only by drawing it. This carries the same information as
/// data, so an editor can underline the mistake or a language server can turn it into a range.
///
/// Positions are **byte offsets**, not line/column. Every consumer derives line and column the way
/// it counts them — an editor in characters, a language server in UTF-16 units — and a byte offset
/// is the one form that is unambiguous for all of them. It also sidesteps a flaw in the rendered
/// trace, whose caret is placed by byte count but padded by character count, so any non-ASCII
/// earlier on the line pushes it to the right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VplParseError {
	/// Byte range of the offending input.
	///
	/// This is the tightest span the parser can justify: the single character that could not be
	/// parsed, or an empty range at the end of the input when the input simply stopped early.
	/// Widen it via [`context`](Self::context) if you want the enclosing construct.
	pub span: Range<usize>,
	/// The innermost failure, e.g. `"expected '=', found k"`.
	pub message: String,
	/// The context stack, innermost first and outermost last.
	///
	/// Never empty, so a consumer can always compose `message` with the construct it happened in.
	/// The one failure `nom` raises outside every frame — input following an otherwise complete
	/// pipeline — carries a single `parsing pipeline` frame at offset 0 rather than no stack at
	/// all. That frame is not drawn in the [`std::fmt::Display`] trace, which shows only what `nom`
	/// itself recorded.
	pub context: Vec<VplErrorFrame>,
	/// The caret-annotated trace, rendered eagerly because [`Display`](std::fmt::Display) has no access to the input.
	trace: String,
}

impl VplParseError {
	/// Extracts the position and context that `nom` accumulated while backtracking.
	///
	/// `error`'s entries each hold the *remaining* input at that point, which is always a suffix
	/// of `input`, so the byte offset is the difference between the two. Entry order is innermost
	/// first, because `nom` pushes each enclosing frame as the failure propagates outwards.
	pub(crate) fn from_verbose(input: &str, error: &VerboseError<&str>) -> Self {
		let offset_of = |remaining: &str| input.offset(remaining);

		let (message, span) = error.errors.first().map_or_else(
			// `nom` never produces an empty error, but the type permits it and a panic here would
			// be a poor trade for a diagnostic.
			|| (String::from("parsing failed"), input.len()..input.len()),
			|(remaining, kind)| {
				let at = offset_of(remaining);
				let span = remaining.chars().next().map_or(at..at, |c| at..at + c.len_utf8());
				(describe(remaining, kind), span)
			},
		);

		let context = error
			.errors
			.iter()
			.filter_map(|(remaining, kind)| match kind {
				VerboseErrorKind::Context(label) => Some(VplErrorFrame {
					label: (*label).to_string(),
					offset: offset_of(remaining),
				}),
				_ => None,
			})
			.collect::<Vec<_>>();
		let context = if context.is_empty() { whole_pipeline() } else { context };

		let trace = render_trace(input, error);
		VplParseError {
			span,
			message,
			context,
			trace,
		}
	}

	/// Builds an error at the end of the input, for failures that carry no position of their own.
	pub(crate) fn at_end_of_input(input: &str, message: impl Into<String>) -> Self {
		let message = message.into();
		VplParseError {
			span: input.len()..input.len(),
			trace: message.clone(),
			message,
			context: whole_pipeline(),
		}
	}
}

/// The stack for a failure `nom` recorded no frames of its own for.
///
/// Only unattached input reaches this: the pipeline parsed to completion and something followed it
/// that could not belong to it, so `all_consuming` raised the failure above every `context(…)`
/// call. The input is still meant to be one pipeline starting at offset 0, and that is what the
/// leftover is measured against, so the stack is that single frame rather than nothing. It is not
/// drawn in [`Display`](std::fmt::Display), which shows only what `nom` itself recorded.
fn whole_pipeline() -> Vec<VplErrorFrame> {
	vec![VplErrorFrame {
		label: String::from("parsing pipeline"),
		offset: 0,
	}]
}

impl fmt::Display for VplParseError {
	/// Writes the caret-annotated trace, i.e. what the CLI prints.
	///
	/// The structured fields are the reason this type exists; this is what it looks like to
	/// someone reading a terminal.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.trace)
	}
}

impl std::error::Error for VplParseError {}

/// Renders the caret-annotated trace: one stanza per entry, innermost first.
///
/// A port of `nom_language::error::convert_error`, kept faithful to it — the wording, the stanza
/// order and the blank line between stanzas are all deliberate matches — with one fix. That
/// function derives the caret's column from a **byte** offset and then pads it with `{:>width$}`,
/// which counts **characters**; so on a line containing anything non-ASCII the caret drifts right
/// by one column for every extra byte those characters occupy. Counting the prefix in characters
/// puts it back under the character it is pointing at.
fn render_trace(input: &str, error: &VerboseError<&str>) -> String {
	// Writing to a `String` cannot fail, so the results below are deliberately discarded.
	use std::fmt::Write;

	let mut result = String::new();

	for (i, (remaining, kind)) in error.errors.iter().enumerate() {
		if input.is_empty() {
			let what = match kind {
				VerboseErrorKind::Char(c) => format!("expected '{c}'"),
				VerboseErrorKind::Context(label) => format!("in {label}"),
				VerboseErrorKind::Nom(kind) => format!("in {kind:?}"),
			};
			let _ = write!(result, "{i}: {what}, got empty input\n\n");
			continue;
		}

		let offset = input.offset(remaining);
		let prefix = &input[..offset];
		let line_number = prefix.matches('\n').count() + 1;
		let line_begin = prefix.rfind('\n').map_or(0, |pos| pos + 1);
		let line = input[line_begin..]
			.lines()
			.next()
			.unwrap_or(&input[line_begin..])
			.trim_end();
		// Characters, not bytes — this is the fix described above.
		let column = prefix[line_begin..].chars().count() + 1;

		let _ = match kind {
			VerboseErrorKind::Char(c) => match remaining.chars().next() {
				Some(actual) => write!(
					result,
					"{i}: at line {line_number}:\n{line}\n{:>column$}\nexpected '{c}', found {actual}\n\n",
					'^'
				),
				None => write!(
					result,
					"{i}: at line {line_number}:\n{line}\n{:>column$}\nexpected '{c}', got end of input\n\n",
					'^'
				),
			},
			VerboseErrorKind::Context(label) => write!(
				result,
				"{i}: at line {line_number}, in {label}:\n{line}\n{:>column$}\n\n",
				'^'
			),
			// A described kind reads as a message under the caret, like the `Char` stanzas above.
			// An undescribed one keeps the old `in <kind>` shape rather than being dressed up.
			VerboseErrorKind::Nom(kind) => match describe_kind(*kind) {
				Some(description) => write!(
					result,
					"{i}: at line {line_number}:\n{line}\n{:>column$}\n{description}\n\n",
					'^'
				),
				None => write!(
					result,
					"{i}: at line {line_number}, in {kind:?}:\n{line}\n{:>column$}\n\n",
					'^'
				),
			},
		};
	}

	result
}

/// Renders the innermost error entry.
fn describe(remaining: &str, kind: &VerboseErrorKind) -> String {
	match kind {
		VerboseErrorKind::Char(expected) => remaining.chars().next().map_or_else(
			|| format!("expected '{expected}', got end of input"),
			|actual| format!("expected '{expected}', found {actual}"),
		),
		VerboseErrorKind::Context(label) => (*label).to_string(),
		VerboseErrorKind::Nom(kind) => describe_kind(*kind).map_or_else(|| format!("{kind:?}"), ToString::to_string),
	}
}

/// Plain wording for the `nom` kinds this grammar can actually produce.
///
/// Only these six are reachable — established by driving the parser with malformed input rather
/// than by reading the combinator list — and each is worded for what was wrong at that position,
/// not for which combinator noticed. Anything unmapped keeps its `nom` name: a name is unhelpful,
/// but a confident guess about an unfamiliar failure would be worse.
fn describe_kind(kind: ErrorKind) -> Option<&'static str> {
	Some(match kind {
		// `all_consuming` found input it could not attach to the pipeline.
		ErrorKind::Eof => "unexpected input",
		// A character-level parser rejected the character under the caret. Deliberately vague
		// about which one: the three are interchangeable from the reader's point of view.
		ErrorKind::OneOf | ErrorKind::Tag | ErrorKind::TakeWhile1 => "unexpected character",
		// Wrappers. These sit above one of the above at the same position, so they rarely carry
		// the useful detail, but they still appear as stanzas in the trace.
		// `Escaped` is raised by this crate's own string scanner, which knows the failure is an
		// escape rather than merely an unexpected byte.
		ErrorKind::Escaped => "malformed escape sequence",
		ErrorKind::Alt => "none of the alternatives matched",
		ErrorKind::Many1 => "expected at least one",
		_ => return None,
	})
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;
	use crate::vpl::parse_vpl_detailed;

	/// Parses `input`, expecting failure, and returns the structured error.
	fn error_of(input: &str) -> VplParseError {
		parse_vpl_detailed(input).expect_err("expected a parse error")
	}

	fn labels(error: &VplParseError) -> Vec<&str> {
		error.context.iter().map(|f| f.label.as_str()).collect()
	}

	#[test]
	fn span_points_at_the_offending_character() {
		let input = "node child key=value ]";
		let error = error_of(input);

		assert_eq!(error.message, "expected '=', found k");
		assert_eq!(&input[error.span.clone()], "k");
		assert_eq!(error.span, 11..12);
		assert_eq!(labels(&error), ["parsing property", "parsing node", "parsing pipeline"]);
	}

	#[test]
	fn span_is_empty_when_the_input_stops_early() {
		let input = "node key=\"2.1";
		let error = error_of(input);

		assert_eq!(error.message, "expected '\"', got end of input");
		assert_eq!(error.span, input.len()..input.len());
		assert!(input[error.span.clone()].is_empty());
	}

	/// The whole point of the exercise: the rendered trace draws its caret two columns right of
	/// the `!` here, because it pads by characters a column it counted in bytes. The offset is
	/// unaffected.
	#[test]
	fn offsets_are_bytes_and_survive_non_ascii() {
		let input = "node a=\"Grüße\" b=!";
		let error = error_of(input);

		assert_eq!(&input[error.span.clone()], "!");
		assert_eq!(error.span, 19..20);
		// 19 bytes but only 17 characters precede the `!` — `ü` and `ß` contribute one extra each.
		assert_eq!(input[..error.span.start].chars().count(), 17);
	}

	/// Frames reach back to where each construct began, so a consumer can widen the span itself.
	#[test]
	fn frames_carry_the_start_of_each_construct() {
		let input = "node key=[n key=2,1]";
		let error = error_of(input);

		let offsets = error.context.iter().map(|f| f.offset).collect::<Vec<_>>();
		assert_eq!(
			labels(&error),
			["parsing array", "parsing property", "parsing node", "parsing pipeline"]
		);
		assert_eq!(offsets, vec![9, 5, 0, 0]);
		// widening to an enclosing construct is then the caller's arithmetic: from where the
		// property started, through to the character that broke it
		assert_eq!(&input[error.span.clone()], "k");
		assert_eq!(&input[offsets[1]..error.span.end], "key=[n k");
	}

	/// A trailing separator is the typo of issue #258, and the one a person editing a pipeline is
	/// most likely to make. The node after a `|` is mandatory, so the failure is reported inside
	/// the node that is missing — with the frames — rather than against the separator.
	#[rstest]
	#[case("node a=1 | ")]
	#[case("node a=1 |")]
	#[case("node a=1 | | node")]
	fn a_trailing_separator_reports_inside_the_missing_node(#[case] input: &str) {
		let error = error_of(input);

		assert_eq!(error.message, "unexpected character");
		assert_eq!(
			labels(&error),
			[
				"parsing name",
				"parsing node identifier",
				"parsing node",
				"parsing pipeline"
			]
		);
		// against what follows the `|`, not against the `|` itself
		assert!(error.span.start > input.find('|').unwrap());
	}

	/// Input that follows an otherwise complete pipeline is the one failure `nom` raises outside
	/// every frame, because nothing was in progress when `all_consuming` rejected it. It is
	/// attributed to the pipeline as a whole so that `context` is never empty. See issue #258.
	#[rstest]
	#[case("node [ ] [ ]", "[")]
	#[case("node [n key=2]]", "]")]
	#[case("node [ child key=value ] node", "n")]
	fn trailing_input_is_attributed_to_the_whole_pipeline(#[case] input: &str, #[case] at: &str) {
		let error = error_of(input);

		assert_eq!(error.message, "unexpected input");
		assert_eq!(&input[error.span.clone()], at);
		assert_eq!(labels(&error), ["parsing pipeline"]);
		assert_eq!(error.context[0].offset, 0);
	}

	/// The guarantee the frames are worth relying on: whatever goes wrong, there is something to
	/// compose the message with.
	#[rstest]
	fn context_is_never_empty(#[values(0)] _dummy: usize) {
		for input in [
			"",
			" ",
			"node | | node",
			"node a=1 | ",
			"node [ ] [ ]",
			"node [n key=2]]",
			"node child key=value ]",
			"node key=\"2.1",
			"node a=/data/berlin.mbtiles",
			"| node",
		] {
			assert!(!error_of(input).context.is_empty(), "no context for {input:?}");
		}
	}

	#[test]
	fn empty_input_is_reported_at_offset_zero() {
		let error = error_of("");
		assert_eq!(error.span, 0..0);
	}

	/// `parse_vpl` keeps its `anyhow` signature, but the position is no longer lost on the way out:
	/// the structured error is still in the chain.
	#[test]
	fn parse_vpl_carries_the_structured_error() {
		let error = crate::vpl::parse_vpl("node child key=value ]").unwrap_err();
		let structured = error
			.chain()
			.find_map(|cause| cause.downcast_ref::<VplParseError>())
			.expect("structured error should be recoverable from the chain");

		assert_eq!(structured.span, 11..12);
		assert_eq!(structured.message, "expected '=', found k");
	}

	/// Returns the `^` line of the given stanza, and how many characters precede the caret.
	fn caret_column(rendered: &str, stanza: usize) -> usize {
		let caret = rendered
			.lines()
			.filter(|l| l.trim() == "^")
			.nth(stanza)
			.expect("stanza should have a caret line");
		caret.chars().count()
	}

	/// The caret is padded by character but was positioned by byte, so anything non-ASCII earlier
	/// on the line used to shove it to the right. Here `!` is the 18th character but the 20th byte.
	#[test]
	fn caret_lands_under_the_offending_character_despite_non_ascii() {
		let input = "node a=\"Grüße\" b=!";
		let rendered = error_of(input).to_string();

		let expected = input.chars().position(|c| c == '!').unwrap() + 1;
		assert_eq!(expected, 18);
		assert_eq!(caret_column(&rendered, 0), expected);
		// the whole line is echoed above the caret, unchanged
		assert_eq!(rendered.lines().nth(1).unwrap(), input);
	}

	/// The port has to rediscover which line the offset falls on, and report each frame against
	/// its own line — the outermost frame here points back at line 1.
	#[test]
	fn line_numbers_follow_the_offset() {
		let rendered = error_of("node a=1 |\nnode b=!").to_string();

		assert!(rendered.starts_with("0: at line 2:\nnode b=!\n"), "{rendered}");
		assert_eq!(caret_column(&rendered, 0), 8);
		assert!(
			rendered.contains("at line 1, in parsing pipeline:\nnode a=1 |\n"),
			"{rendered}"
		);
	}

	/// Empty input takes a separate branch with its own wording and no caret at all.
	#[test]
	fn empty_input_renders_without_a_caret() {
		let rendered = error_of("").to_string();

		assert!(rendered.contains("got empty input"), "{rendered}");
		assert!(!rendered.contains('^'), "{rendered}");
	}

	/// Both entry points render the same trace, so preferring the detailed one costs nothing.
	#[test]
	fn both_entry_points_render_alike() {
		let input = "node key=\"2.1";
		let rendered = crate::vpl::parse_vpl(input)
			.unwrap_err()
			.chain()
			.last()
			.unwrap()
			.to_string();

		assert_eq!(error_of(input).to_string(), rendered);
		assert!(rendered.contains("expected '\"', got end of input"));
	}
}
