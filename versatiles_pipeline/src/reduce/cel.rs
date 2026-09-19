//! Writing CEL that `vector_filter_features` can evaluate without erroring.
//!
//! Everything here exists because an expression that fails to evaluate drops the feature, and the
//! reduction contract forbids dropping a feature the style draws. So the rules are narrower than
//! "produce valid CEL": the output has to be **total** — defined for every value a tile can hold.
//!
//! Three facts about `cel-interpreter 0.10` shape all of it, each measured rather than assumed
//! (see the tests in `operations/vector/vector_filter_features.rs`):
//!
//! - `==`, `!=` and `in` compare across types and return `false`; ordering comparisons error.
//! - `has(props.x)` works for an identifier-safe key; `has(props['a:b'])` does not compile, so a
//!   key that is not an identifier tests presence with `'a:b' in props`.
//! - There is no `type()`, so `is_num` — an extension this crate registers — is the only way to
//!   keep an ordering comparison from erroring on a mistyped value.

use std::fmt::Write;

use super::ir::Literal;

/// CEL's keywords, which cannot follow a `.` even though they look like identifiers.
///
/// A property named `in` or `function` is unlikely but not impossible in OSM-derived data, and
/// `props.in` is a parse error rather than a wrong answer — the kind of failure that appears as a
/// whole pipeline refusing to build, far from the tag that caused it.
const CEL_KEYWORDS: &[&str] = &[
	"as",
	"break",
	"const",
	"continue",
	"else",
	"false",
	"for",
	"function",
	"if",
	"import",
	"in",
	"let",
	"loop",
	"namespace",
	"null",
	"package",
	"return",
	"true",
	"var",
	"void",
	"while",
];

/// Whether a property name can follow a `.` in CEL.
fn is_identifier(name: &str) -> bool {
	!name.is_empty()
		&& !name.starts_with(|c: char| c.is_ascii_digit())
		&& name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
		&& !CEL_KEYWORDS.contains(&name)
}

/// A single-quoted CEL string literal.
///
/// Only `\` and `'` need escaping inside single quotes; the control characters get their named
/// escapes so the rendered pipeline stays one line and stays readable.
pub fn string_literal(value: &str) -> String {
	let mut out = String::with_capacity(value.len() + 2);
	out.push('\'');
	for c in value.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\'' => out.push_str("\\'"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if c.is_control() => {
				// `\u` with four hex digits, which CEL accepts and a terminal will not mangle.
				let _ = write!(out, "\\u{:04x}", c as u32);
			}
			c => out.push(c),
		}
	}
	out.push('\'');
	out
}

/// A literal as CEL source.
pub fn literal(value: &Literal) -> String {
	match value {
		Literal::Bool(b) => b.to_string(),
		Literal::Number(n) => Literal::render_number(*n),
		Literal::String(s) => string_literal(s),
	}
}

/// Reading a property: `props.kind`, or `props['addr:street']` when the name is not an identifier.
pub fn property(name: &str) -> String {
	if is_identifier(name) {
		format!("props.{name}")
	} else {
		format!("props[{}]", string_literal(name))
	}
}

/// Testing that a property is present.
///
/// Two spellings because `has()` takes a field selection and nothing else: `has(props['a:b'])` is
/// a compile error, not a `false`.
pub fn has_property(name: &str) -> String {
	if is_identifier(name) {
		format!("has(props.{name})")
	} else {
		format!("{} in props", string_literal(name))
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	#[test]
	fn identifier_safe_names_use_the_dot_form() {
		assert_eq!(property("kind"), "props.kind");
		assert_eq!(property("name_de"), "props.name_de");
		assert_eq!(property("a1"), "props.a1");
		assert_eq!(has_property("kind"), "has(props.kind)");
	}

	#[test]
	fn other_names_use_the_index_form() {
		// OSM keys carry colons constantly, and `props.addr:street` does not parse.
		assert_eq!(property("addr:street"), "props['addr:street']");
		assert_eq!(property("name:de"), "props['name:de']");
		assert_eq!(property("has-dash"), "props['has-dash']");
		assert_eq!(property(""), "props['']");
		assert_eq!(property("1st"), "props['1st']", "cannot start with a digit");
	}

	#[test]
	fn a_keyword_is_not_an_identifier() {
		// `props.in` is a parse error, which surfaces as the whole pipeline refusing to build.
		assert_eq!(property("in"), "props['in']");
		assert_eq!(property("true"), "props['true']");
		assert_eq!(has_property("function"), "'function' in props");
	}

	#[test]
	fn presence_of_a_non_identifier_uses_in_rather_than_has() {
		// `has(props['a:b'])` is "invalid argument to has() macro" — a compile error, so this is
		// not a stylistic choice.
		assert_eq!(has_property("addr:street"), "'addr:street' in props");
	}

	#[test]
	fn string_literals_escape_what_would_end_them() {
		assert_eq!(string_literal("plain"), "'plain'");
		assert_eq!(string_literal("it's"), r"'it\'s'");
		assert_eq!(string_literal(r"back\slash"), r"'back\\slash'");
		assert_eq!(string_literal("two\nlines"), r"'two\nlines'");
		assert_eq!(string_literal("tab\there"), r"'tab\there'");
		// A control character with no named escape still must not reach the output as a raw byte.
		assert_eq!(string_literal("bell\u{7}"), "'bell\\u0007'");
	}

	#[test]
	fn literals_render_by_type() {
		assert_eq!(literal(&Literal::Bool(true)), "true");
		assert_eq!(literal(&Literal::Bool(false)), "false");
		assert_eq!(literal(&Literal::Number(1000.0)), "1000", "not 1000.0");
		assert_eq!(literal(&Literal::Number(2.5)), "2.5");
		assert_eq!(literal(&Literal::String("x".to_string())), "'x'");
	}
}
