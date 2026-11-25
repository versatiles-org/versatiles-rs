//! The operators, functions and macros a feature expression can use, for an editor to offer.
//!
//! [`expression_names`](super::feature_expr::expression_names) lists the `$` names an
//! expression reads. This lists what it can do with them. The functions are those of
//! `cel-interpreter`'s default context in this build (without the `chrono` feature, so no
//! `timestamp` or `duration`), plus `is_num`, which this crate adds.
//! A test compiles and evaluates every entry's [`example`](ExpressionOperator::example), so an
//! entry that stops working breaks the build instead of reaching an editor.

/// How an entry of [`expression_operators`] is written, which tells an editor where the
/// operands go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionOperatorKind {
	/// Compares two values, written between them: `a == b`.
	Comparison,
	/// Combines or chooses between conditions: `a && b`, `!a`, `c ? a : b`.
	Logic,
	/// Computes a number or joins strings and lists, written between the operands: `a + b`.
	Arithmetic,
	/// Takes its arguments in parentheses: `size(x)`.
	Function,
	/// Is called on a value: `x.startsWith('a')`.
	Method,
	/// Is called on a list with a variable and an expression, `list.all(x, x > 0)`, or tests a
	/// field, `has(m.f)`.
	Macro,
}

impl ExpressionOperatorKind {
	/// The kind as a lowercase word, such as `"comparison"`, for an editor that groups entries.
	#[must_use]
	pub const fn as_str(self) -> &'static str {
		match self {
			ExpressionOperatorKind::Comparison => "comparison",
			ExpressionOperatorKind::Logic => "logic",
			ExpressionOperatorKind::Arithmetic => "arithmetic",
			ExpressionOperatorKind::Function => "function",
			ExpressionOperatorKind::Method => "method",
			ExpressionOperatorKind::Macro => "macro",
		}
	}
}

/// One operator, function or macro a feature expression can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpressionOperator {
	/// As it is inserted, such as `>=`, `size` or `startsWith`. The conditional is `? :`.
	pub written: &'static str,
	/// Where its operands go.
	pub kind: ExpressionOperatorKind,
	/// What it does, in one sentence.
	pub doc: &'static str,
	/// An expression using it that evaluates to `true`, such as `size('abc') == 3`.
	pub example: &'static str,
}

const fn entry(
	written: &'static str,
	kind: ExpressionOperatorKind,
	doc: &'static str,
	example: &'static str,
) -> ExpressionOperator {
	ExpressionOperator {
		written,
		kind,
		doc,
		example,
	}
}

use ExpressionOperatorKind::{Arithmetic, Comparison, Function, Logic, Macro, Method};

/// Every entry, grouped by kind in the order an editor would list them.
const EXPRESSION_OPERATORS: [ExpressionOperator; 35] = [
	entry(
		"==",
		Comparison,
		"Whether two values are equal; values of different types are not.",
		"1 == 1",
	),
	entry(
		"!=",
		Comparison,
		"Whether two values differ; values of different types do.",
		"1 != 2",
	),
	entry(
		"<",
		Comparison,
		"Whether a value is less than another; fails for values of different types.",
		"1 < 2",
	),
	entry(
		"<=",
		Comparison,
		"Whether a value is at most another; fails for values of different types.",
		"1 <= 1",
	),
	entry(
		">",
		Comparison,
		"Whether a value is greater than another; fails for values of different types.",
		"2 > 1",
	),
	entry(
		">=",
		Comparison,
		"Whether a value is at least another; fails for values of different types.",
		"1 >= 1",
	),
	entry(
		"in",
		Comparison,
		"Whether a value is in a list, or a key in a map.",
		"'a' in ['a', 'b']",
	),
	entry(
		"&&",
		Logic,
		"Whether both conditions hold; stops at the first that is false.",
		"true && true",
	),
	entry(
		"||",
		Logic,
		"Whether either condition holds; stops at the first that is true.",
		"false || true",
	),
	entry("!", Logic, "Negates a condition.", "!false"),
	entry(
		"? :",
		Logic,
		"Chooses the second value if the condition holds, otherwise the third.",
		"(1 > 0 ? 'a' : 'b') == 'a'",
	),
	entry(
		"+",
		Arithmetic,
		"Adds numbers, or joins strings or lists.",
		"1 + 2 == 3 && 'a' + 'b' == 'ab'",
	),
	entry(
		"-",
		Arithmetic,
		"Subtracts a number, or negates one.",
		"3 - 1 == 2 && -1 < 0",
	),
	entry("*", Arithmetic, "Multiplies numbers.", "2 * 3 == 6"),
	entry(
		"/",
		Arithmetic,
		"Divides numbers; dividing integers drops the remainder.",
		"7 / 2 == 3",
	),
	entry("%", Arithmetic, "The remainder of dividing integers.", "7 % 2 == 1"),
	entry(
		"size",
		Function,
		"The length of a string, list or map.",
		"size('abc') == 3",
	),
	entry("max", Function, "The largest of its arguments.", "max(1, 3, 2) == 3"),
	entry("min", Function, "The smallest of its arguments.", "min(1, 3, 2) == 1"),
	entry("string", Function, "Converts a value to a string.", "string(1) == '1'"),
	entry(
		"int",
		Function,
		"Converts a string or number to an integer; fails if it cannot.",
		"int('42') == 42",
	),
	entry(
		"uint",
		Function,
		"Converts a string or number to an unsigned integer; fails if it cannot.",
		"uint(42) == 42u",
	),
	entry(
		"double",
		Function,
		"Converts a string or number to a floating-point number; fails if it cannot.",
		"double('1.5') == 1.5",
	),
	entry("bytes", Function, "Converts a string to bytes.", "bytes('a') == b'a'"),
	entry(
		"is_num",
		Function,
		"Whether a value is a number; answers rather than failing, so it can guard a comparison.",
		"is_num(1) && !is_num('1')",
	),
	entry(
		"contains",
		Method,
		"Whether a string contains another, or a list a value.",
		"'abc'.contains('b')",
	),
	entry(
		"startsWith",
		Method,
		"Whether a string starts with another.",
		"'abc'.startsWith('a')",
	),
	entry(
		"endsWith",
		Method,
		"Whether a string ends with another.",
		"'abc'.endsWith('c')",
	),
	entry(
		"matches",
		Method,
		"Whether a string matches a regular expression.",
		"'abc'.matches('^a.c$')",
	),
	entry(
		"has",
		Macro,
		"Whether a map has a field, without failing when it does not.",
		"has({'a': 1}.a) && !has({'a': 1}.b)",
	),
	entry(
		"all",
		Macro,
		"Whether a condition holds for every element of a list.",
		"[1, 2].all(x, x > 0)",
	),
	entry(
		"exists",
		Macro,
		"Whether a condition holds for at least one element of a list.",
		"[1, 2].exists(x, x > 1)",
	),
	entry(
		"exists_one",
		Macro,
		"Whether a condition holds for exactly one element of a list.",
		"[1, 2].exists_one(x, x > 1)",
	),
	entry(
		"map",
		Macro,
		"The list of an expression's value for every element of a list.",
		"[1, 2].map(x, x * 2) == [2, 4]",
	),
	entry(
		"filter",
		Macro,
		"The elements of a list for which a condition holds.",
		"[1, 2].filter(x, x > 1) == [2]",
	),
];

/// Every operator, function and macro a feature expression can use, with what it does and an
/// example.
///
/// For an editor offering them as completions or in a reference, beside
/// [`expression_names`](super::feature_expr::expression_names): these are what this build of the
/// expression language has, which is less than the CEL spec lists. Each entry's example compiles
/// and evaluates to `true`, which a test checks. The expressions are those of
/// `vector_branch expr=` and `vector_filter_features expr=`.
#[must_use]
pub fn expression_operators() -> &'static [ExpressionOperator] {
	&EXPRESSION_OPERATORS
}

#[cfg(test)]
mod tests {
	use versatiles_core::TileCoord;

	use super::*;
	use crate::helpers::feature_expr::FeatureExpr;

	fn holds(expr: &str) -> bool {
		let compiled = FeatureExpr::new(expr).unwrap_or_else(|e| panic!("{expr}: {e:#}"));
		compiled.evaluate_tile(&TileCoord::new(0, 0, 0).unwrap())
	}

	/// An editor offers only what works: every example compiles and is `true`.
	#[test]
	fn every_example_compiles_and_holds() {
		for op in expression_operators() {
			assert!(holds(op.example), "{}: `{}` is not true", op.written, op.example);
		}
	}

	/// Each example uses what it stands for, so it tests that entry and not another.
	#[test]
	fn every_example_uses_its_entry() {
		for op in expression_operators() {
			let used = match op.written {
				"? :" => op.example.contains('?') && op.example.contains(':'),
				written => op.example.contains(written),
			};
			assert!(used, "{}: `{}` does not use it", op.written, op.example);
		}
	}

	#[test]
	fn every_doc_is_one_sentence() {
		for op in expression_operators() {
			assert!(op.doc.ends_with('.'), "{}: {:?}", op.written, op.doc);
			assert!(op.doc.starts_with(char::is_uppercase), "{}: {:?}", op.written, op.doc);
			assert_eq!(op.doc.matches(". ").count(), 0, "{}: {:?}", op.written, op.doc);
		}
	}

	/// The list as an editor sees it. A change here is a change to what editors offer, so it
	/// shows in review.
	#[test]
	fn the_list_is_as_committed() {
		let listed: Vec<String> = expression_operators()
			.iter()
			.map(|op| format!("{} {}", op.kind.as_str(), op.written))
			.collect();
		assert_eq!(
			listed,
			[
				"comparison ==",
				"comparison !=",
				"comparison <",
				"comparison <=",
				"comparison >",
				"comparison >=",
				"comparison in",
				"logic &&",
				"logic ||",
				"logic !",
				"logic ? :",
				"arithmetic +",
				"arithmetic -",
				"arithmetic *",
				"arithmetic /",
				"arithmetic %",
				"function size",
				"function max",
				"function min",
				"function string",
				"function int",
				"function uint",
				"function double",
				"function bytes",
				"function is_num",
				"method contains",
				"method startsWith",
				"method endsWith",
				"method matches",
				"macro has",
				"macro all",
				"macro exists",
				"macro exists_one",
				"macro map",
				"macro filter",
			]
		);
	}

	/// The CEL spec has functions this build leaves out. If one starts to work — because
	/// `cel-interpreter` gains it or a feature such as `chrono` is turned on — it is missing from
	/// the list, and this fails so that it is added. `cel-interpreter` doesn't say which
	/// functions its default context has, so this is the closest check there is.
	#[test]
	fn functions_of_the_spec_this_build_lacks_do_not_work() {
		for expr in [
			"timestamp('2026-01-01T00:00:00Z') != null",
			"duration('1s') != null",
			"type(1) == int",
			"'abc'.lowerAscii() == 'abc'",
			"'a,b'.split(',') == ['a', 'b']",
		] {
			let works =
				FeatureExpr::new(expr).is_ok_and(|compiled| compiled.evaluate_tile(&TileCoord::new(0, 0, 0).unwrap()));
			assert!(!works, "`{expr}` works: add what it uses to expression_operators()");
		}
	}
}
