//! What a style needs from a tileset, as types the renderer can walk.
//!
//! Built by [`style::from_style`](super::style::from_style) from a MapLibre style and consumed by
//! [`render::operations`](super::render::operations), which turns it into filter operations. It is
//! an internal intermediate and nothing serialises it: the analysis and the rendering are two
//! halves of one crate, so the shape between them answers to nothing but them.
//!
//! ## The rule that makes partial support safe
//!
//! The requirement is a **conservative over-approximation**: it may name features the style never
//! draws, but never omits one it does. A consumer reading only layer names is correct; one that
//! also reads `properties` is correct and produces smaller tiles; one that reads the predicates too
//! is correct and produces smaller tiles still. Every "I could not read this" path in the analysis
//! widens rather than narrows — see the module docs of
//! [`style::filter`](super::style) for the asymmetry that entails.

use std::collections::BTreeMap;

/// A style's data requirement: which layers, properties and features it draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Requirement {
	/// One entry per source-layer the style reads. A layer absent from this map is one the style
	/// never draws.
	pub layers: BTreeMap<String, LayerRequirement>,
}

/// What one source-layer has to keep.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRequirement {
	/// Property names the style reads from this layer, sorted. Empty means the layer is drawn using
	/// geometry alone, so every property can go.
	pub properties: Vec<String>,
	/// Zoom-scoped predicates, **OR-ed together**: a feature is needed if any entry matches.
	pub keep: Vec<KeepEntry>,
}

/// One (zoom range, predicate) pair. Every part is optional, and absent means "unbounded".
///
/// Zoom sits beside the predicate rather than inside it because the two describe different things
/// — a tile coordinate and a feature's properties — even though `vector_filter_features` can now
/// express both in one expression.
#[derive(Debug, Clone, PartialEq)]
pub struct KeepEntry {
	/// Lowest zoom this entry applies to. Absent means "from the bottom of the pyramid".
	pub minzoom: Option<u8>,
	/// Highest zoom this entry applies to. Absent means "to the top of the pyramid".
	pub maxzoom: Option<u8>,
	/// The predicate. Absent means "keep everything in this zoom range" — rule 2, and the value
	/// every widening arrives at.
	pub predicate: Option<Predicate>,
}

/// A literal a predicate compares against.
///
/// JSON has one number type, so this does too. [`Literal::render_number`] is where an integral
/// value is written back without a decimal point.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
	/// `true` / `false`.
	Bool(bool),
	/// Any JSON number.
	Number(f64),
	/// A string.
	String(String),
}

/// One node of a requirement's filter expression.
///
/// The operator set is closed, and genuinely so: the only thing that builds a `Predicate` is
/// [`style::filter::normalize`](super::style), which answers `None` for anything outside this set
/// rather than inventing a member. That is why there is no `Unknown` variant — the widening happens
/// one level up, in the `Option`, where the caller can see it.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
	/// Every argument must match.
	And(Vec<Predicate>),
	/// At least one argument must match.
	Or(Vec<Predicate>),
	/// The arguments, taken together, must not match.
	Not(Vec<Predicate>),
	/// `field == value`.
	Eq(String, Literal),
	/// `field != value`.
	Ne(String, Literal),
	/// `field` is one of the values.
	In(String, Vec<Literal>),
	/// `field` is none of the values.
	Nin(String, Vec<Literal>),
	/// `field < value`.
	Lt(String, Literal),
	/// `field <= value`.
	Le(String, Literal),
	/// `field > value`.
	Gt(String, Literal),
	/// `field >= value`.
	Ge(String, Literal),
	/// The feature carries `field` at all.
	Has(String),
}

impl Literal {
	/// Writes a number the way the source wrote it, so `1000` does not become `1000.0`.
	///
	/// JSON does not distinguish the two and neither does the parser, so an integral value is
	/// recovered here rather than remembered. Values beyond `i64` keep the float spelling.
	#[must_use]
	#[expect(
		clippy::cast_possible_truncation,
		reason = "the guard admits only whole numbers below 2^53, which i64 holds exactly"
	)]
	pub fn render_number(value: f64) -> String {
		if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
			format!("{}", value as i64)
		} else {
			format!("{value}")
		}
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	#[test]
	fn whole_numbers_render_without_a_decimal_point() {
		// `props.population >= 1000.0` is legal CEL but reads as a mistake, and the style wrote an
		// integer.
		assert_eq!(Literal::render_number(1000.0), "1000");
		assert_eq!(Literal::render_number(-3.0), "-3");
		assert_eq!(Literal::render_number(0.0), "0");
	}

	#[test]
	fn fractional_numbers_keep_their_spelling() {
		assert_eq!(Literal::render_number(1.5), "1.5");
	}

	#[test]
	fn numbers_beyond_i64_do_not_saturate() {
		// `1e300 as i64` saturates to `i64::MAX`, which would render a comparison against
		// 9223372036854775807 — a different question than the style asked.
		let rendered = Literal::render_number(1e300);
		assert_ne!(rendered, i64::MAX.to_string());
		assert!((rendered.parse::<f64>().unwrap() - 1e300).abs() < f64::EPSILON * 1e300);
	}
}
