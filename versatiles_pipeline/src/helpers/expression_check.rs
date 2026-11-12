//! What an editor asks about a feature expression as it is typed.

use super::{
	expression_tokens::{ExpressionToken, lex},
	feature_expr::{ExpressionProblem, ExpressionReads, compile_checked, reads_of},
};

/// What [`check_expression`] finds in an expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionCheck {
	/// What is wrong with it, each with where. Empty when it compiles.
	pub problems: Vec<ExpressionProblem>,
	/// Its tokens, for highlighting. There even when it does not compile.
	pub tokens: Vec<ExpressionToken>,
	/// What it reads, when it compiles.
	pub reads: Option<ExpressionReads>,
}

impl ExpressionCheck {
	/// Whether the expression compiles.
	#[must_use]
	pub fn compiles(&self) -> bool {
		self.problems.is_empty()
	}
}

/// Checks a `vector_branch` or `vector_filter_features` expression, as an
/// editor asks while someone types it.
///
/// The answer is structured where building's is text: each problem has its
/// own message and the characters it covers, the tokens say what to colour,
/// and a compiling expression says which names it reads and how much of a
/// tile its answer depends on. It uses the compiler building uses, so an
/// expression it accepts builds, and one it refuses fails to build with the
/// same messages.
///
/// The tokens come from a lexer pass, which never fails. Compiling an
/// expression parses it, which takes a little longer, and for an expression
/// that does not compile it may also print to the terminal: see
/// `compile_checked`.
#[must_use]
pub fn check_expression(text: &str) -> ExpressionCheck {
	let tokens = lex(text).into_iter().map(|lexed| lexed.token).collect();
	match compile_checked(text) {
		Ok(program) => ExpressionCheck {
			problems: Vec::new(),
			tokens,
			reads: Some(reads_of(&program)),
		},
		Err(problems) => ExpressionCheck {
			problems,
			tokens,
			reads: None,
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::{expression_tokens::ExpressionTokenKind, feature_expr::ExpressionScope};

	#[test]
	fn a_compiling_expression_says_what_it_reads() {
		let check = check_expression("$layer == 'poi' && $zoom > 12");
		assert!(check.compiles());
		let reads = check.reads.unwrap();
		assert_eq!(reads.names, ["$zoom", "$layer"]);
		assert_eq!(reads.properties, Vec::<String>::new());
		assert_eq!(reads.scope, ExpressionScope::Layer);

		let reads = check_expression("kind == 'a' && population > 1 && has($props.name)")
			.reads
			.unwrap();
		assert_eq!(reads.names, ["$props"]);
		assert_eq!(reads.properties, ["kind", "population"]);
		assert_eq!(reads.scope, ExpressionScope::Feature);

		assert_eq!(check_expression("$x == 1").reads.unwrap().scope, ExpressionScope::Tile);
	}

	/// The positions an editor underlines, counted in characters of the text
	/// as written, for each kind of problem.
	#[test]
	fn each_problem_says_where_it_is() {
		let span = |text: &str| {
			let check = check_expression(text);
			assert!(check.reads.is_none(), "{text}");
			let problem = &check.problems[0];
			let covered: String = text
				.chars()
				.skip(problem.start)
				.take(problem.end - problem.start)
				.collect();
			(problem.start, problem.end, covered, problem.message.clone())
		};

		// The parser's own errors, at the token they are about.
		let (start, end, covered, message) = span("foo bar");
		assert_eq!((start, end, covered.as_str()), (4, 7, "bar"));
		assert!(message.starts_with("Syntax error: extraneous input 'bar'"), "{message}");
		assert!(
			!message.contains("<input>") && !message.contains("versatiles help"),
			"{message}"
		);

		// A character no expression can contain, which can crash the parser.
		let (start, end, covered, message) = span("kind == @");
		assert_eq!((start, end, covered.as_str()), (8, 9, "@"));
		assert_eq!(message, "Syntax error: `@` cannot appear in an expression");

		// After a `$` name the column still counts the `$`.
		let (start, ..) = span("$zoom > 1 && (kind");
		assert_eq!(start, 18);

		// Missing after a trailing operator: an empty span just past it.
		assert_eq!(span("$layer =="), (9, 9, String::new(), span("$layer ==").3));
		// An unclosed string, from its quote to the end.
		assert_eq!(span("kind == 'ab").2, "'ab");
		// A `$` name that does not exist, and a reserved one.
		assert_eq!(span("$zom > 1").2, "$zom");
		assert_eq!(span("_zoom > 1").2, "_zoom");
		// A former bare name, where it is read as a variable.
		assert_eq!(span("$props.zoom == 1 || zoom > 1").2, "zoom");
		assert_eq!(span("$props.zoom == 1 || zoom > 1").0, 20);
	}

	#[test]
	fn tokens_come_even_when_it_does_not_compile() {
		let check = check_expression("$zoom >= 12 && kind ==");
		assert!(!check.compiles());
		let kinds: Vec<ExpressionTokenKind> = check.tokens.iter().map(|token| token.kind).collect();
		assert_eq!(
			kinds,
			[
				ExpressionTokenKind::Special,
				ExpressionTokenKind::Operator,
				ExpressionTokenKind::Number,
				ExpressionTokenKind::Operator,
				ExpressionTokenKind::Property,
				ExpressionTokenKind::Operator,
			]
		);
	}

	/// What the editor is told and what building says can't differ.
	#[test]
	fn building_fails_with_the_same_messages() {
		for text in ["kind == @", "$layer ==", "$zom > 1", "zoom > 1", "(kind"] {
			let check = check_expression(text);
			let built = format!(
				"{:#}",
				crate::helpers::feature_expr::CelExpression::try_from(text).unwrap_err()
			);
			for problem in &check.problems {
				assert!(built.contains(&problem.message), "{text}: {built}");
			}
		}
	}
}
