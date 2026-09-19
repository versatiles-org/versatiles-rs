//! Normalising a MapLibre filter into the closed predicate vocabulary the renderer can emit.
//!
//! MapLibre filters are expressions — an open grammar with arithmetic, string operations and
//! data-driven lookups — but the renderer has to evaluate the result per feature in CEL, totally
//! and without a fallback. So what crosses this boundary is a small closed set:
//!
//! ```text
//! and · or · not · eq · ne · in · nin · lt · le · gt · ge · has
//! ```
//!
//! Classifying all 318 filters in the deployed `colorful` style, the cartography uses exactly the
//! leaf shapes below and three combinators, and every one maps onto that set. The widening
//! machinery exists for drift and for foreign styles, not for anything shipping today.
//!
//! ## The direction everything widens in
//!
//! The reduction may keep features the style never draws; it must never drop one it does. `None`
//! is this module's way of saying *I could not normalise this*, and the caller reads it as **keep
//! everything** — the widest answer available.
//!
//! That makes the combinators asymmetric, which is the one genuinely subtle thing here:
//!
//! - `all` **may drop** a conjunct it cannot read. Removing a constraint from a conjunction widens.
//! - `any` **may not**. Dropping a disjunct removes a whole reason to keep a feature, which
//!   narrows. One unreadable disjunct poisons the entire `or`.
//! - `not` may not widen its child at all. Negating a widened predicate narrows the result, so an
//!   unreadable child makes the whole negation unreadable.
//!
//! Get one of those backwards and the failure mode is not an error — it is a tileset that quietly
//! lost features, noticed as a missing label on a map months later.

use versatiles_core::json::JsonValue;

use crate::reduce::ir::{Literal, Predicate};

/// Builds the IR node for one comparison operator.
type Comparison = fn(String, Literal) -> Predicate;

/// Comparison operators, in both the modern and the legacy spelling, to their IR constructor.
const COMPARISONS: &[(&str, Comparison)] = &[
	("==", Predicate::Eq),
	("!=", Predicate::Ne),
	("<", Predicate::Lt),
	("<=", Predicate::Le),
	(">", Predicate::Gt),
	(">=", Predicate::Ge),
];

/// The comparisons whose IR form insists on a number rather than any literal.
const NUMERIC: &[&str] = &["<", "<=", ">", ">="];

/// The field a filter operand names, in either spelling.
///
/// The modern grammar wraps it (`["get", "kind"]`); the legacy filter grammar puts a bare string in
/// position 1 (`["==", "kind", "x"]`, `["has", "service"]`). Both occur in the deployed styles — 45
/// of `colorful`'s filters use the legacy `has` form — so both are read.
///
/// A bare string is only a field name because this is a *filter*. In a general expression it would
/// be a string literal, and reading it as a field would be wrong — which is why this is not used
/// outside filter operand positions.
fn field_of(value: &JsonValue) -> Option<String> {
	match value {
		JsonValue::String(name) => Some(name.clone()),
		JsonValue::Array(array) => match array.as_vec().as_slice() {
			[JsonValue::String(op), JsonValue::String(name)] if op == "get" => Some(name.clone()),
			_ => None,
		},
		_ => None,
	}
}

/// A literal scalar, or `None` for anything that has to be evaluated.
///
/// `null` is rejected rather than carried: MapLibre's equality against `null` and a tile pipeline's
/// notion of an absent property are not reliably the same thing, and the IR has no way to say
/// "whatever MapLibre means here". Rejecting it widens, which is always safe.
fn literal_of(value: &JsonValue) -> Option<Literal> {
	match value {
		JsonValue::Boolean(b) => Some(Literal::Bool(*b)),
		JsonValue::Number(n) => Some(Literal::Number(*n)),
		JsonValue::String(s) => Some(Literal::String(s.clone())),
		_ => None,
	}
}

/// The literal list of an `in` test, in either the modern `["literal", […]]` or the legacy vararg
/// form.
fn values_of(args: &[JsonValue]) -> Option<Vec<Literal>> {
	// `["in", ["get","kind"], ["literal", ["a","b"]]]` — one argument that is the wrapped list.
	let list: &[JsonValue] = match args {
		[JsonValue::Array(array)] => match array.as_vec().as_slice() {
			[JsonValue::String(op), JsonValue::Array(items)] if op == "literal" => items.as_vec(),
			_ => args,
		},
		_ => args,
	};

	// One unreadable member would make the set smaller than the filter's, which narrows.
	list.iter().map(literal_of).collect()
}

/// `and` / `or` over already-normalised arguments, unwrapping the one-argument case.
fn combine(args: Vec<Predicate>, build: fn(Vec<Predicate>) -> Predicate) -> Option<Predicate> {
	match args.len() {
		0 => None,
		1 => args.into_iter().next(),
		_ => Some(build(args)),
	}
}

/// A MapLibre filter as a [`Predicate`], or `None` when it cannot be expressed in the closed
/// vocabulary — which the caller must read as **keep every feature**, never as "keep none".
///
/// Total over the expression grammar: anything unrecognised widens rather than failing, because a
/// reduction that refuses to build is worse than one that keeps too much.
pub fn normalize(filter: &JsonValue) -> Option<Predicate> {
	let JsonValue::Array(array) = filter else { return None };
	let items = array.as_vec();
	let [JsonValue::String(op), args @ ..] = items.as_slice() else {
		return None;
	};

	match op.as_str() {
		// `all` may drop what it cannot read; each dropped conjunct only widens the result.
		"all" => combine(args.iter().filter_map(normalize).collect(), Predicate::And),

		// `any` may not. A disjunct is a reason to keep a feature, and dropping one narrows.
		"any" => {
			let parts = args.iter().map(normalize).collect::<Option<Vec<_>>>()?;
			combine(parts, Predicate::Or)
		}

		// Legacy `none` is `not(any(…))`, and inherits `any`'s strictness through the negation.
		"none" => {
			let parts = args.iter().map(normalize).collect::<Option<Vec<_>>>()?;
			combine(parts, Predicate::Or).map(|inner| Predicate::Not(vec![inner]))
		}

		"!" => match args {
			// Negation cannot widen: `not` of a widened child is narrower, not wider.
			[arg] => normalize(arg).map(|p| Predicate::Not(vec![p])),
			_ => None,
		},

		"has" | "!has" => match args {
			[arg] => {
				let has = Predicate::Has(field_of(arg)?);
				Some(if op == "has" { has } else { Predicate::Not(vec![has]) })
			}
			_ => None,
		},

		// `["to-boolean", ["get", f]]` is a truthiness test, and `has` is the closest the closed set
		// has. They are **not** equivalent: a field present but empty (`""`, `0`) passes `has` and
		// fails `to-boolean`. So this is a deliberate widening — `has` keeps a superset — and it is
		// why the operator set stays closed without a `truthy` member. The deployed styles use it
		// for the POI category fields (`amenity`, `shop`, `leisure`, …), whose values are non-empty
		// strings, so the two agree there in practice anyway.
		"to-boolean" => match args {
			[arg] => Some(Predicate::Has(field_of(arg)?)),
			_ => None,
		},

		"in" | "!in" => {
			let [field, rest @ ..] = args else { return None };
			if rest.is_empty() {
				return None;
			}
			let values = values_of(rest)?;
			let test = if op == "in" {
				Predicate::In(field_of(field)?, values)
			} else {
				Predicate::Nin(field_of(field)?, values)
			};
			Some(test)
		}

		_ => {
			let (_, build) = COMPARISONS.iter().find(|(name, _)| name == op)?;
			let [field, value] = args else { return None };
			let value = literal_of(value)?;
			// `["<", ["get","x"], "a"]` is a string comparison in MapLibre and does not mean the
			// numeric `lt` the renderer would emit, so it widens rather than mistranslating.
			if NUMERIC.contains(&op.as_str()) && !matches!(value, Literal::Number(_)) {
				return None;
			}
			Some(build(field_of(field)?, value))
		}
	}
}

/// Every field a predicate reads.
///
/// The caller checks these against the properties it exports: a `where` that reads a property the
/// reduction strips would evaluate against a feature that no longer carries it.
pub fn fields(predicate: &Predicate, out: &mut std::collections::BTreeSet<String>) {
	match predicate {
		Predicate::And(args) | Predicate::Or(args) | Predicate::Not(args) => {
			for arg in args {
				fields(arg, out);
			}
		}
		Predicate::Has(field)
		| Predicate::Eq(field, _)
		| Predicate::Ne(field, _)
		| Predicate::Lt(field, _)
		| Predicate::Le(field, _)
		| Predicate::Gt(field, _)
		| Predicate::Ge(field, _)
		| Predicate::In(field, _)
		| Predicate::Nin(field, _) => {
			out.insert(field.clone());
		}
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	fn norm(json: &str) -> Option<Predicate> {
		normalize(&JsonValue::parse_str(json).unwrap())
	}

	#[test]
	fn reads_both_field_spellings() {
		let modern = norm(r#"["==",["get","kind"],"motorway"]"#);
		let legacy = norm(r#"["==","kind","motorway"]"#);
		assert_eq!(modern, legacy);
		assert_eq!(
			modern,
			Some(Predicate::Eq("kind".into(), Literal::String("motorway".into())))
		);
	}

	#[test]
	fn reads_both_in_spellings() {
		let modern = norm(r#"["in",["get","kind"],["literal",["a","b"]]]"#);
		let legacy = norm(r#"["in","kind","a","b"]"#);
		assert_eq!(modern, legacy);
		assert_eq!(
			modern,
			Some(Predicate::In(
				"kind".into(),
				vec![Literal::String("a".into()), Literal::String("b".into())]
			))
		);
	}

	#[test]
	fn all_drops_an_unreadable_conjunct() {
		// Removing a constraint from a conjunction widens, so this is allowed.
		assert_eq!(
			norm(r#"["all",["has","a"],["sorcery","b"]]"#),
			Some(Predicate::Has("a".into()))
		);
	}

	#[test]
	fn any_is_poisoned_by_an_unreadable_disjunct() {
		// A disjunct is a reason to keep a feature; dropping one would narrow.
		assert_eq!(norm(r#"["any",["has","a"],["sorcery","b"]]"#), None);
	}

	#[test]
	fn not_is_poisoned_by_an_unreadable_child() {
		// `not` of a widened child is narrower, not wider.
		assert_eq!(norm(r#"["!",["sorcery","a"]]"#), None);
		assert_eq!(norm(r#"["none",["sorcery","a"]]"#), None);
	}

	#[test]
	fn none_is_a_negated_any() {
		assert_eq!(
			norm(r#"["none",["has","a"],["has","b"]]"#),
			Some(Predicate::Not(vec![Predicate::Or(vec![
				Predicate::Has("a".into()),
				Predicate::Has("b".into())
			])]))
		);
	}

	#[test]
	fn to_boolean_widens_to_has() {
		// Deliberate: a present-but-empty value passes `has` and fails `to-boolean`, so `has` keeps
		// a superset — which is the safe direction.
		assert_eq!(
			norm(r#"["to-boolean",["get","amenity"]]"#),
			Some(Predicate::Has("amenity".into()))
		);
	}

	#[test]
	fn an_ordering_against_a_string_widens() {
		// MapLibre compares strings here; the renderer would emit a numeric comparison, which is a
		// different question. Widening beats mistranslating.
		assert_eq!(norm(r#"[">=",["get","population"],"1000"]"#), None);
		assert_eq!(
			norm(r#"[">=",["get","population"],1000]"#),
			Some(Predicate::Ge("population".into(), Literal::Number(1000.0)))
		);
	}

	#[test]
	fn a_null_comparison_widens() {
		// MapLibre's `null` and an absent tile property are not reliably the same thing.
		assert_eq!(norm(r#"["==",["get","kind"],null]"#), None);
	}

	#[test]
	fn an_unreadable_in_member_widens() {
		// Keeping the readable members would make the set smaller than the filter's, which narrows.
		assert_eq!(norm(r#"["in",["get","k"],["literal",["a",["get","b"]]]]"#), None);
	}

	#[test]
	fn a_single_argument_combinator_unwraps() {
		assert_eq!(norm(r#"["all",["has","a"]]"#), Some(Predicate::Has("a".into())));
	}

	#[test]
	fn an_empty_combinator_widens() {
		assert_eq!(norm(r#"["all"]"#), None);
		assert_eq!(norm(r#"["any"]"#), None);
	}

	#[test]
	fn a_non_array_filter_widens() {
		assert_eq!(norm(r#""kind""#), None);
		assert_eq!(norm("true"), None);
		assert_eq!(norm(r#"[["get","k"],1]"#), None);
	}

	#[test]
	fn negated_forms_round_trip() {
		assert_eq!(
			norm(r#"["!has","a"]"#),
			Some(Predicate::Not(vec![Predicate::Has("a".into())]))
		);
		assert_eq!(
			norm(r#"["!in","k","a"]"#),
			Some(Predicate::Nin("k".into(), vec![Literal::String("a".into())]))
		);
		assert_eq!(
			norm(r#"["!=","k",true]"#),
			Some(Predicate::Ne("k".into(), Literal::Bool(true)))
		);
	}

	#[test]
	fn predicate_fields_collects_every_field() {
		let p = norm(r#"["all",["has","a"],["any",["==","b",1],["!",["in","c","x"]]]]"#).unwrap();
		let mut out = std::collections::BTreeSet::new();
		fields(&p, &mut out);
		assert_eq!(out.into_iter().collect::<Vec<_>>(), ["a", "b", "c"]);
	}
}
