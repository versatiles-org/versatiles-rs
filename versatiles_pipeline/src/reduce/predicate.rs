//! Turning a requirement's predicate into a CEL expression.
//!
//! ## Widening has a direction, and negation reverses it
//!
//! The contract is that the pipeline may keep too much and must never drop what the style draws,
//! so anything this cannot express exactly becomes "keep". That is straightforward until a `not`
//! is in the way: substituting `true` for an unknown predicate *inside* a negation renders as
//! `!(true)`, which is `false`, which drops every feature of that layer. The safe substitution
//! flips with the polarity of the position it sits in.
//!
//! So [`render`] carries `widen`: the value an inexpressible sub-predicate takes *here*. It starts
//! `true` and inverts under every `not`. Every widening in this file goes through it rather than
//! writing a literal `true`, because a literal `true` is only correct in half the positions.

use super::{
	cel,
	ir::{Literal, Predicate},
};

/// Renders a predicate as CEL.
///
/// `widen` is what a sub-predicate becomes when it cannot be expressed exactly — `true` in a
/// positive position, `false` under an odd number of negations. Callers start with `true`.
pub fn render(predicate: &Predicate, widen: bool) -> String {
	match predicate {
		// An operator from a newer exporter. Nothing is known about what it matches, so it takes
		// the value that keeps the most in this position.
		Predicate::Unknown => widen.to_string(),

		Predicate::Has(field) => cel::has_property(field),

		// Equality and membership never error, so they need no guard against a mistyped value —
		// an int simply is not equal to a string. The `has` guard is still there so that a
		// *missing* property reads as "not equal" rather than as `null == 'x'`.
		Predicate::Eq(field, value) => guarded(field, "==", value),
		Predicate::In(field, values) => guarded_in(field, values),

		// Negated forms wrap the positive one rather than flipping the operator: `!(has(k) &&
		// k == 'v')` keeps a feature that lacks `k`, while `has(k) && k != 'v'` drops it. The
		// first is the widening direction, which is the one the contract asks for.
		Predicate::Ne(field, value) => format!("!({})", guarded(field, "==", value)),
		Predicate::Nin(field, values) => format!("!({})", guarded_in(field, values)),

		Predicate::Lt(field, value) => ordering(field, "<", value, widen),
		Predicate::Le(field, value) => ordering(field, "<=", value, widen),
		Predicate::Gt(field, value) => ordering(field, ">", value, widen),
		Predicate::Ge(field, value) => ordering(field, ">=", value, widen),

		Predicate::And(args) => join(args, "&&", widen),
		Predicate::Or(args) => join(args, "||", widen),

		// The negation is where `widen` inverts. `not` with several arguments negates their
		// conjunction, which is how the format's `args` reads everywhere else.
		Predicate::Not(args) => format!("!({})", join(args, "&&", !widen)),
	}
}

/// `has(k) && k <op> v`, for the operators that cannot error.
fn guarded(field: &str, op: &str, value: &Literal) -> String {
	format!(
		"({} && {} {op} {})",
		cel::has_property(field),
		cel::property(field),
		cel::literal(value)
	)
}

/// `has(k) && k in [...]`.
fn guarded_in(field: &str, values: &[Literal]) -> String {
	let list = values.iter().map(cel::literal).collect::<Vec<_>>().join(", ");
	format!("({} && {} in [{list}])", cel::has_property(field), cel::property(field))
}

/// An ordering comparison, which is the only family that errors on a type mismatch.
///
/// Two guards, and the second is not optional: `has()` short-circuits only when the property is
/// *absent*, so a property that is present and holds a string still reaches the comparison and
/// still errors. `is_num` is what keeps that from dropping the feature.
///
/// A non-numeric literal is not a comparison this can guard at all — `is_num(k) && k >= '10'`
/// passes the guard and then compares a number against a string — so the whole predicate widens.
fn ordering(field: &str, op: &str, value: &Literal, widen: bool) -> String {
	if !matches!(value, Literal::Number(_)) {
		return widen.to_string();
	}
	format!(
		"({} && is_num({}) && {} {op} {})",
		cel::has_property(field),
		cel::property(field),
		cel::property(field),
		cel::literal(value)
	)
}

/// Joins arguments with `&&` or `||`, widening an empty list.
///
/// An empty `and` is conventionally `true` and an empty `or` conventionally `false`, but neither
/// is a thing the exporter means to say — an empty argument list is a requirement that failed to
/// describe itself, so it widens like any other gap.
fn join(args: &[Predicate], op: &str, widen: bool) -> String {
	if args.is_empty() {
		return widen.to_string();
	}
	let parts: Vec<String> = args.iter().map(|a| render(a, widen)).collect();
	if parts.len() == 1 {
		return parts.into_iter().next().unwrap();
	}
	format!("({})", parts.join(&format!(" {op} ")))
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	fn s(text: &str) -> Literal {
		Literal::String(text.to_string())
	}

	fn render_positive(p: &Predicate) -> String {
		render(p, true)
	}

	#[test]
	fn has_needs_no_guard() {
		assert_eq!(render_positive(&Predicate::Has("name".to_string())), "has(props.name)");
		assert_eq!(
			render_positive(&Predicate::Has("addr:street".to_string())),
			"'addr:street' in props"
		);
	}

	#[test]
	fn equality_is_guarded_for_presence_only() {
		assert_eq!(
			render_positive(&Predicate::Eq("kind".to_string(), s("motorway"))),
			"(has(props.kind) && props.kind == 'motorway')"
		);
	}

	#[test]
	fn inequality_keeps_features_lacking_the_property() {
		// `!(has(k) && k == v)` is true when k is absent; `has(k) && k != v` is false. The format
		// over-approximates, so the first is the correct rendering.
		assert_eq!(
			render_positive(&Predicate::Ne("bridge".to_string(), Literal::Bool(true))),
			"!((has(props.bridge) && props.bridge == true))"
		);
	}

	#[test]
	fn membership_renders_a_list() {
		assert_eq!(
			render_positive(&Predicate::In("kind".to_string(), vec![s("motorway"), s("trunk")])),
			"(has(props.kind) && props.kind in ['motorway', 'trunk'])"
		);
	}

	#[test]
	fn ordering_carries_the_numeric_guard() {
		assert_eq!(
			render_positive(&Predicate::Ge("population".to_string(), Literal::Number(1000.0))),
			"(has(props.population) && is_num(props.population) && props.population >= 1000)"
		);
	}

	#[test]
	fn ordering_against_a_non_numeric_literal_widens() {
		// `is_num(k) && k >= '10'` would pass the guard and then error comparing number to
		// string, so there is nothing to guard — the predicate cannot be expressed.
		assert_eq!(
			render_positive(&Predicate::Ge("population".to_string(), s("1000"))),
			"true"
		);
		assert_eq!(
			render(&Predicate::Ge("population".to_string(), s("1000")), false),
			"false",
			"and it follows polarity like any other widening"
		);
	}

	#[test]
	fn conjunction_and_disjunction_nest() {
		let p = Predicate::And(vec![
			Predicate::Has("name".to_string()),
			Predicate::Or(vec![
				Predicate::Eq("kind".to_string(), s("city")),
				Predicate::Eq("kind".to_string(), s("town")),
			]),
		]);
		assert_eq!(
			render_positive(&p),
			"(has(props.name) && ((has(props.kind) && props.kind == 'city') || (has(props.kind) && props.kind == 'town')))"
		);
	}

	#[test]
	fn a_single_argument_needs_no_extra_parentheses() {
		let p = Predicate::And(vec![Predicate::Has("name".to_string())]);
		assert_eq!(render_positive(&p), "has(props.name)");
	}

	#[test]
	fn an_unknown_operator_widens_to_keep() {
		assert_eq!(render_positive(&Predicate::Unknown), "true");
	}

	#[test]
	fn widening_inverts_under_a_negation() {
		// The bug this exists to prevent: rendering the inner `Unknown` as `true` would make the
		// whole thing `!(true)` — `false` — and drop every feature of the layer, which is the one
		// outcome the contract forbids.
		assert_eq!(render_positive(&Predicate::Not(vec![Predicate::Unknown])), "!(false)");
	}

	#[test]
	fn widening_inverts_again_under_two_negations() {
		let p = Predicate::Not(vec![Predicate::Not(vec![Predicate::Unknown])]);
		assert_eq!(render_positive(&p), "!(!(true))");
	}

	#[test]
	fn widening_inside_a_negated_conjunction_still_keeps_everything() {
		// `not(has(k) and <unknown>)`: the unknown must not make the conjunction *more* true,
		// because the negation would then drop more.
		let p = Predicate::Not(vec![Predicate::And(vec![
			Predicate::Has("k".to_string()),
			Predicate::Unknown,
		])]);
		assert_eq!(render_positive(&p), "!((has(props.k) && false))");
	}

	#[test]
	fn an_empty_argument_list_widens() {
		assert_eq!(render_positive(&Predicate::And(vec![])), "true");
		assert_eq!(render_positive(&Predicate::Or(vec![])), "true");
		assert_eq!(render_positive(&Predicate::Not(vec![])), "!(false)");
	}
}
