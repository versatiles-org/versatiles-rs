//! Structured VPL parse errors.
//!
//! Nothing here interprets the grammar: the offsets and labels are `nom`'s own, extracted rather
//! than invented. The extraction is possible because every entry in a `VerboseError` holds the
//! input still remaining at that point, which is a suffix of the whole input — so the difference
//! in length is the byte offset. `convert_error` computes exactly this before discarding it in
//! favour of a rendered string.

use nom::Offset;
use nom_language::error::{VerboseError, VerboseErrorKind};
use std::ops::Range;

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
	pub context: Vec<VplErrorFrame>,
}

impl VplParseError {
	/// Extracts the position and context that `nom` accumulated while backtracking.
	///
	/// `error`'s entries each hold the *remaining* input at that point, which is always a suffix
	/// of `input`, so the byte offset is the difference between the two. Entry order is innermost
	/// first, because `nom` pushes each enclosing frame as the failure propagates outwards.
	// Constructed by `parse_vpl` once the structured error is wired into it; see issue #217.
	#[allow(dead_code)]
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
			.collect();

		VplParseError { span, message, context }
	}
}

/// Renders the innermost error entry, wording `Char` failures as the trace does.
///
/// `Nom` kinds are passed through under their own names (`"Eof"`, `"OneOf"`); turning those into
/// something a user can act on is a separate job from extracting them.
fn describe(remaining: &str, kind: &VerboseErrorKind) -> String {
	match kind {
		VerboseErrorKind::Char(expected) => remaining.chars().next().map_or_else(
			|| format!("expected '{expected}', got end of input"),
			|actual| format!("expected '{expected}', found {actual}"),
		),
		VerboseErrorKind::Context(label) => (*label).to_string(),
		VerboseErrorKind::Nom(kind) => format!("{kind:?}"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vpl::parser::parse_vpl_verbose;

	/// Parses `input`, expecting failure, and returns the structured error.
	fn error_of(input: &str) -> VplParseError {
		let error = parse_vpl_verbose(input).expect_err("expected a parse error");
		VplParseError::from_verbose(input, &error)
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

	/// Failures from `all_consuming` carry no context at all — a correct span with a `nom`
	/// kind for a message. Recorded so the gap is visible rather than surprising.
	#[test]
	fn some_failures_have_no_context_at_all() {
		let input = "node | | node";
		let error = error_of(input);

		assert_eq!(error.message, "Eof");
		assert_eq!(&input[error.span.clone()], "|");
		assert!(error.context.is_empty());
	}

	#[test]
	fn empty_input_is_reported_at_offset_zero() {
		let error = error_of("");
		assert_eq!(error.span, 0..0);
	}
}
