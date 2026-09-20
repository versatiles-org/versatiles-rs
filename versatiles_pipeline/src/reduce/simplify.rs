//! Shrinking a predicate without changing what it matches.
//!
//! Cartography is written one style layer per case — one for motorways, one for trunk roads, one
//! for each of those in a tunnel — and the analysis ORs every layer sharing a source-layer and a
//! zoom range into a single predicate. That is correct and enormous: the deployed `colorful` style
//! turns `streets` into a 232-term disjunction, about 31 kB of CEL with 1,277 boolean operators.
//!
//! Size alone would be forgivable. Depth is not. `cel-parser` builds a left-leaning binary tree and
//! both parsing and evaluation recurse over it, so a disjunction of a thousand terms is a thousand
//! stack frames — which is fine on a Linux main thread with 8 MB and a crash on Windows, where the
//! main thread gets 1 MB. That is not a hypothetical: it is what `Windows: Test` failed on.
//!
//! ## Every rule here is exact
//!
//! Nothing in this module widens or narrows; the simplified predicate matches exactly the features
//! the original did. That matters because the reduction's contract is one-directional and a
//! "simplification" that quietly dropped a term would be indistinguishable from the bug the whole
//! design exists to prevent. Three rules, each an identity:
//!
//! - **Flattening.** `a || (b || c)` is `a || b || c`. The analysis nests these when it merges a
//!   layer whose own filter was already a disjunction.
//! - **Deduplication.** `a || a` is `a`. Two style layers drawing the same features at the same
//!   zooms with the same filter — `runway` and `taxiway` each appear twice in `colorful`'s
//!   `streets` — contribute the same term twice.
//! - **Membership merging.** `k == 'a' || k == 'b'` is `k in ['a', 'b']`, and `k in [..] || k == c`
//!   extends the list. This is the big one, because one-layer-per-value is how the cartography is
//!   written.
//!
//! Factoring a shared conjunct — `(k == 'a' && t) || (k == 'b' && t)` into `k in ['a','b'] && t` —
//! is also an identity and would shrink `colorful` further. It is deliberately not done: it is the
//! first rule here whose correctness is not obvious at a glance, and the three above already bring
//! the expression well under the depth that matters. Add it when a style needs it, with tests.

use std::collections::BTreeMap;

use super::ir::{Literal, Predicate};

/// Rewrites a predicate into a smaller one matching exactly the same features.
#[must_use]
pub fn simplify(predicate: Predicate) -> Predicate {
	match predicate {
		Predicate::And(args) => combine(args, false),
		Predicate::Or(args) => combine(args, true),
		Predicate::Not(args) => Predicate::Not(args.into_iter().map(simplify).collect()),
		leaf => leaf,
	}
}

/// Simplifies the arguments of an `and` / `or`, then flattens, deduplicates and merges them.
///
/// `is_or` selects which node type absorbs its own kind when flattening — `a && (b && c)` and
/// `a || (b || c)` collapse, but `a && (b || c)` must not.
fn combine(args: Vec<Predicate>, is_or: bool) -> Predicate {
	let mut flat: Vec<Predicate> = Vec::new();
	for arg in args {
		match simplify(arg) {
			Predicate::Or(inner) if is_or => flat.extend(inner),
			Predicate::And(inner) if !is_or => flat.extend(inner),
			other => flat.push(other),
		}
	}

	// Order-preserving rather than sort-and-dedup: the rendered expression is meant to be read and
	// edited, and keeping it in the order the style declared its layers is what makes it possible
	// to find the layer a term came from. Quadratic, over a few hundred terms at worst.
	let mut unique: Vec<Predicate> = Vec::with_capacity(flat.len());
	for predicate in flat {
		if !unique.contains(&predicate) {
			unique.push(predicate);
		}
	}

	if is_or {
		unique = merge_memberships(unique);
	}

	match unique.len() {
		// Neither can arise from the analysis, which never builds an empty combinator. An empty
		// `and` is conventionally true and an empty `or` false, and rendering either as a leaf
		// would be a widening or a narrowing made silently — so they keep their node and let the
		// renderer's own empty-list handling decide, which already documents the choice.
		0 => {
			if is_or {
				Predicate::Or(Vec::new())
			} else {
				Predicate::And(Vec::new())
			}
		}
		1 => unique.into_iter().next().unwrap(),
		_ if is_or => Predicate::Or(unique),
		_ => Predicate::And(unique),
	}
}

/// Merges the `eq` and `in` tests of a disjunction, one list per field.
///
/// Only sound inside an `or`: `k == 'a' || k == 'b'` is a membership test, while `k == 'a' && k ==
/// 'b'` is a contradiction and merging it would invert its meaning.
fn merge_memberships(args: Vec<Predicate>) -> Vec<Predicate> {
	let mut out: Vec<Predicate> = Vec::new();
	// Field name → where its accumulating `in` sits in `out`, so the merged test keeps the position
	// of the first term that mentioned the field.
	let mut slot: BTreeMap<String, usize> = BTreeMap::new();

	for predicate in args {
		let (field, values) = match predicate {
			Predicate::Eq(field, value) => (field, vec![value]),
			Predicate::In(field, values) => (field, values),
			other => {
				out.push(other);
				continue;
			}
		};

		if let Some(&index) = slot.get(&field) {
			let Predicate::In(_, existing) = &mut out[index] else {
				unreachable!("slot only ever points at an In")
			};
			for value in values {
				if !existing.contains(&value) {
					existing.push(value);
				}
			}
		} else {
			slot.insert(field.clone(), out.len());
			out.push(Predicate::In(field, dedup(values)));
		}
	}

	// A field mentioned once is still an equality, and `k == 'a'` reads better than `k in ['a']`.
	for predicate in &mut out {
		if let Predicate::In(field, values) = predicate
			&& values.len() == 1
		{
			*predicate = Predicate::Eq(field.clone(), values[0].clone());
		}
	}

	out
}

/// Order-preserving deduplication of a literal list.
fn dedup(values: Vec<Literal>) -> Vec<Literal> {
	let mut out: Vec<Literal> = Vec::with_capacity(values.len());
	for value in values {
		if !out.contains(&value) {
			out.push(value);
		}
	}
	out
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	fn eq(field: &str, value: &str) -> Predicate {
		Predicate::Eq(field.to_string(), Literal::String(value.to_string()))
	}

	fn has(field: &str) -> Predicate {
		Predicate::Has(field.to_string())
	}

	fn values(names: &[&str]) -> Vec<Literal> {
		names.iter().map(|n| Literal::String((*n).to_string())).collect()
	}

	#[test]
	fn equalities_on_one_field_become_a_membership_test() {
		assert_eq!(
			simplify(Predicate::Or(vec![eq("kind", "a"), eq("kind", "b")])),
			Predicate::In("kind".to_string(), values(&["a", "b"]))
		);
	}

	#[test]
	fn a_membership_test_absorbs_further_equalities() {
		let p = Predicate::Or(vec![
			Predicate::In("kind".to_string(), values(&["a", "b"])),
			eq("kind", "c"),
		]);
		assert_eq!(simplify(p), Predicate::In("kind".to_string(), values(&["a", "b", "c"])));
	}

	#[test]
	fn different_fields_stay_apart() {
		// Merging across fields would turn `kind == 'a' || other == 'b'` into nonsense.
		let p = Predicate::Or(vec![eq("kind", "a"), eq("other", "b")]);
		assert_eq!(simplify(p.clone()), p);
	}

	#[test]
	fn duplicate_terms_collapse() {
		// `runway` and `taxiway` each appear twice in colorful's `streets`.
		let p = Predicate::Or(vec![has("a"), has("b"), has("a")]);
		assert_eq!(simplify(p), Predicate::Or(vec![has("a"), has("b")]));
	}

	#[test]
	fn nested_disjunctions_flatten() {
		let p = Predicate::Or(vec![has("a"), Predicate::Or(vec![has("b"), has("c")])]);
		assert_eq!(simplify(p), Predicate::Or(vec![has("a"), has("b"), has("c")]));
	}

	#[test]
	fn nested_conjunctions_flatten() {
		let p = Predicate::And(vec![has("a"), Predicate::And(vec![has("b"), has("c")])]);
		assert_eq!(simplify(p), Predicate::And(vec![has("a"), has("b"), has("c")]));
	}

	#[test]
	fn an_or_inside_an_and_is_not_flattened() {
		// `a && (b || c)` is not `a && b && c`.
		let inner = Predicate::Or(vec![has("b"), has("c")]);
		let p = Predicate::And(vec![has("a"), inner.clone()]);
		assert_eq!(simplify(p), Predicate::And(vec![has("a"), inner]));
	}

	#[test]
	fn equalities_are_not_merged_inside_a_conjunction() {
		// `k == 'a' && k == 'b'` matches nothing; as `k in ['a','b']` it would match both.
		let p = Predicate::And(vec![eq("kind", "a"), eq("kind", "b")]);
		assert_eq!(simplify(p.clone()), p);
	}

	#[test]
	fn a_single_value_stays_an_equality() {
		assert_eq!(simplify(Predicate::Or(vec![eq("kind", "a")])), eq("kind", "a"));
	}

	#[test]
	fn a_repeated_value_appears_once_in_the_list() {
		let p = Predicate::Or(vec![eq("kind", "a"), eq("kind", "b"), eq("kind", "a")]);
		assert_eq!(simplify(p), Predicate::In("kind".to_string(), values(&["a", "b"])));
	}

	#[test]
	fn simplification_reaches_inside_a_negation() {
		let p = Predicate::Not(vec![Predicate::Or(vec![eq("k", "a"), eq("k", "b")])]);
		assert_eq!(
			simplify(p),
			Predicate::Not(vec![Predicate::In("k".to_string(), values(&["a", "b"]))])
		);
	}

	#[test]
	fn compound_terms_keep_their_place_among_merged_ones() {
		// The merged membership test takes the position of the first term that named the field, so
		// the expression still reads in the order the style declared its layers.
		let compound = Predicate::And(vec![eq("kind", "x"), has("tunnel")]);
		let p = Predicate::Or(vec![eq("kind", "a"), compound.clone(), eq("kind", "b")]);
		assert_eq!(
			simplify(p),
			Predicate::Or(vec![Predicate::In("kind".to_string(), values(&["a", "b"])), compound])
		);
	}

	#[test]
	fn an_empty_combinator_keeps_its_node() {
		// Collapsing it to a leaf would be a widening or a narrowing decided here rather than by
		// the renderer, which documents the choice.
		assert_eq!(simplify(Predicate::Or(Vec::new())), Predicate::Or(Vec::new()));
		assert_eq!(simplify(Predicate::And(Vec::new())), Predicate::And(Vec::new()));
	}
}
