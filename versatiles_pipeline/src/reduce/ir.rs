//! The data-requirement format, read into types the renderer can walk.
//!
//! The shape is defined by `versatiles-org/versatiles-style#134`, which emits it; this crate
//! consumes it and renders a pipeline. Everything that knows the JSON's spelling lives in this
//! file, so the exporter changing is one file to update rather than a search.
//!
//! Parsing goes through [`JsonValue`], the workspace's own JSON reader, rather than a derive.
//! The format is small and this is the only place that reads it, so the derive would buy little,
//! and it would put `serde_json::Value` in the public signature of [`KeepEntry::source`] — which
//! ties every consumer of this crate to one `serde_json` version for a field none of them read.
//!
//! ## The rule that makes partial support safe
//!
//! The format is a **conservative over-approximation**: it may name features the style never
//! draws, but never omits one it does. A consumer reading only layer names is correct; one that
//! also reads `properties` is correct and produces smaller tiles; one that reads `where` too is
//! correct and produces smaller tiles still. That is what lets the renderer implement the format
//! in stages, and why every "I do not understand this" path here widens rather than narrows.
//!
//! [`Predicate::Unknown`] is that rule made mechanical: an operator this version has never heard
//! of still parses, and the renderer turns it into "keep".

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use versatiles_core::json::{JsonObject, JsonValue};

/// The only format version this understands.
const SUPPORTED_VERSION: u32 = 1;

/// A style's data requirement: which layers, properties and features it draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Requirement {
	/// Format version. Checked by [`Requirement::from_json`].
	pub version: u32,
	/// Where the requirement came from. Carried for diagnostics; nothing here reads it.
	pub source: Option<SourceInfo>,
	/// One entry per source-layer the style reads. A layer absent from this map is one the
	/// style never draws.
	pub layers: BTreeMap<String, LayerRequirement>,
}

/// Provenance of a requirement, for diagnosing a reduction that went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
	/// The tile schema the style was written against, for example `shortbread`.
	pub schema: Option<String>,
	/// The language the style was built for, which decides which `name:xx` fields it reads.
	pub language: Option<String>,
}

/// What one source-layer has to keep.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRequirement {
	/// Property names the style reads from this layer. Empty means the layer is drawn using
	/// geometry alone, so every property can go.
	pub properties: Vec<String>,
	/// Zoom-scoped predicates, **OR-ed together**: a feature is needed if any entry matches.
	/// An empty list means no feature of this layer is needed at any zoom.
	pub keep: Vec<KeepEntry>,
}

/// One (zoom range, predicate) pair. Both halves are optional.
///
/// Zoom sits beside the predicate rather than inside it because the two may have to render into
/// different places — which was true when the format was written, and stopped being true for this
/// consumer when #274 put `zoom` in the expression language.
#[derive(Debug, Clone, PartialEq)]
pub struct KeepEntry {
	/// Lowest zoom this entry applies to. Absent means "from the bottom".
	pub minzoom: Option<u8>,
	/// Highest zoom this entry applies to. Absent means "to the top".
	pub maxzoom: Option<u8>,
	/// The predicate. Absent means "keep everything in this zoom range".
	pub predicate: Option<Predicate>,
	/// The original MapLibre filter, unnormalised. Kept and deliberately unread: it is what
	/// makes a wrong reduction diagnosable, and discarding it at the parse boundary would put it
	/// out of reach of an error message that wants to quote it.
	pub source: Option<JsonValue>,
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
/// The operator set is closed — `versatiles-style#134` states that the exporter emits nothing
/// else — but [`Predicate::Unknown`] exists anyway, because "closed" is a promise about today's
/// exporter and this has to keep working against tomorrow's.
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
	/// An operator this version does not know.
	///
	/// Not an error: the format's contract is that it over-approximates, so the safe reading of
	/// an unknown operator is "this might match anything", and the renderer widens it to "keep".
	/// Failing instead would turn a format the exporter is allowed to extend into a hard stop.
	Unknown,
}

impl Literal {
	/// Writes a number the way the source wrote it, so `1000` does not become `1000.0`.
	///
	/// JSON does not distinguish the two and neither does [`JsonValue`], so an integral value is
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

/// Reads an optional zoom level, rejecting one that is not a whole number in range.
///
/// `minzoom: 14.5` or `minzoom: 300` is a broken exporter, and `as u8` would silently turn the
/// second into `44` — a zoom range that looks plausible and filters the wrong tiles.
#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "the ensure! above admits only whole numbers in 0..=255"
)]
fn zoom(object: &JsonObject, key: &str) -> Result<Option<u8>> {
	let Some(value) = object.number(key)? else {
		return Ok(None);
	};
	ensure!(
		value.fract() == 0.0 && (0.0..=255.0).contains(&value),
		"{key} must be a whole number between 0 and 255, found {value}"
	);
	Ok(Some(value as u8))
}

impl Literal {
	/// Reads a literal, rejecting a shape that cannot be compared against.
	fn from_value(value: &JsonValue) -> Result<Self> {
		Ok(match value {
			JsonValue::Boolean(b) => Literal::Bool(*b),
			JsonValue::Number(n) => Literal::Number(*n),
			JsonValue::String(s) => Literal::String(s.clone()),
			// A null, array or object where a scalar belongs is malformed rather than unknown:
			// there is no comparison it could have meant.
			other => bail!("expected a string, number or boolean, found {}", other.type_as_str()),
		})
	}
}

impl Predicate {
	/// Reads a predicate, widening an unrecognised operator and rejecting a malformed known one.
	fn from_value(value: &JsonValue) -> Result<Self> {
		let object = value.as_object().context("a predicate must be an object")?;
		let op = object.string("op")?.context("a predicate must carry an `op`")?;

		// The split that matters: an `op` this build does not know widens to `Unknown`, because
		// the exporter is allowed to grow new operators. An `op` it does know but cannot read is
		// an error, because that is a bug, and widening it would hide the bug behind a tileset
		// that is merely larger than it should be — the one symptom nobody investigates.
		let predicate = match op.as_str() {
			"and" => Predicate::And(Self::args(object)?),
			"or" => Predicate::Or(Self::args(object)?),
			"not" => Predicate::Not(Self::args(object)?),
			"eq" => Predicate::Eq(Self::field(object)?, Self::value(object)?),
			"ne" => Predicate::Ne(Self::field(object)?, Self::value(object)?),
			"in" => Predicate::In(Self::field(object)?, Self::values(object)?),
			"nin" => Predicate::Nin(Self::field(object)?, Self::values(object)?),
			"lt" => Predicate::Lt(Self::field(object)?, Self::value(object)?),
			"le" => Predicate::Le(Self::field(object)?, Self::value(object)?),
			"gt" => Predicate::Gt(Self::field(object)?, Self::value(object)?),
			"ge" => Predicate::Ge(Self::field(object)?, Self::value(object)?),
			"has" => Predicate::Has(Self::field(object)?),
			_ => Predicate::Unknown,
		};
		Ok(predicate)
	}

	fn args(object: &JsonObject) -> Result<Vec<Predicate>> {
		let array = object.array("args")?.context("`args` is required")?;
		array
			.iter()
			.map(Predicate::from_value)
			.collect::<Result<Vec<_>>>()
			.context("in `args`")
	}

	fn field(object: &JsonObject) -> Result<String> {
		object.string("field")?.context("`field` is required")
	}

	fn value(object: &JsonObject) -> Result<Literal> {
		let value = object.get("value").context("`value` is required")?;
		Literal::from_value(value).context("in `value`")
	}

	fn values(object: &JsonObject) -> Result<Vec<Literal>> {
		let array = object.array("values")?.context("`values` is required")?;
		array
			.iter()
			.map(Literal::from_value)
			.collect::<Result<Vec<_>>>()
			.context("in `values`")
	}
}

impl KeepEntry {
	fn from_value(value: &JsonValue) -> Result<Self> {
		let object = value.as_object().context("a keep entry must be an object")?;
		Ok(KeepEntry {
			minzoom: zoom(object, "minzoom")?,
			maxzoom: zoom(object, "maxzoom")?,
			predicate: object.get("where").map(Predicate::from_value).transpose()?,
			source: object.get("source").cloned(),
		})
	}
}

impl LayerRequirement {
	fn from_value(value: &JsonValue) -> Result<Self> {
		let object = value.as_object().context("a layer must be an object")?;
		let keep = match object.array("keep")? {
			Some(array) => array
				.iter()
				.map(KeepEntry::from_value)
				.collect::<Result<Vec<_>>>()
				.context("in `keep`")?,
			None => Vec::new(),
		};
		Ok(LayerRequirement {
			properties: object.string_vec("properties")?.unwrap_or_default(),
			keep,
		})
	}
}

impl SourceInfo {
	fn from_object(object: &JsonObject) -> Result<Self> {
		Ok(SourceInfo {
			schema: object.string("schema")?,
			language: object.string("language")?,
		})
	}
}

impl Requirement {
	/// Reads a requirement from JSON, rejecting a version this does not understand.
	///
	/// # Errors
	///
	/// When the JSON does not parse, is not shaped like a requirement, or its `version` is not
	/// [`SUPPORTED_VERSION`].
	pub fn from_json(json: &str) -> Result<Self> {
		let value = JsonValue::parse_str(json).context("Failed to parse the requirement as JSON")?;
		let object = value.as_object().context("a requirement must be a JSON object")?;

		let version = object.number("version")?.context("`version` is required")?;

		// Unlike an unknown operator, an unknown *version* cannot be widened around: a format
		// break is free to change what an existing construct means, and reading v2 as though it
		// were v1 could invert a predicate rather than merely misunderstand it. The whole safety
		// argument rests on the file meaning what this code thinks it means.
		//
		// Compared as text rather than as a float: `1.0 == 1` is a question about representation
		// that the answer here does not depend on.
		let version = Literal::render_number(version);
		if version != SUPPORTED_VERSION.to_string() {
			bail!("unsupported requirement format version {version}, expected {SUPPORTED_VERSION}");
		}

		let layers_object = object.object("layers")?.context("`layers` is required")?;
		let mut layers = BTreeMap::new();
		for (name, value) in layers_object.iter() {
			let layer = LayerRequirement::from_value(value).with_context(|| format!("in layer {name:?}"))?;
			layers.insert(name.clone(), layer);
		}

		Ok(Requirement {
			version: SUPPORTED_VERSION,
			source: object.object("source")?.map(SourceInfo::from_object).transpose()?,
			layers,
		})
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	fn fixture(name: &str) -> String {
		let path = format!("{}/../testdata/reduce/{name}", env!("CARGO_MANIFEST_DIR"));
		std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
	}

	#[test]
	fn reads_the_minimal_requirement() {
		let r = Requirement::from_json(&fixture("minimal.json")).unwrap();
		assert_eq!(r.version, 1);
		assert_eq!(r.layers.len(), 1);

		let layer = &r.layers["water"];
		assert!(layer.properties.is_empty(), "no properties read");
		assert_eq!(layer.keep.len(), 1);
		assert_eq!(layer.keep[0].predicate, None, "no predicate means keep everything");
	}

	#[test]
	fn reads_the_format_documentation_example() {
		// The example from versatiles-style#134, verbatim. If the exporter changes shape, this
		// is the test that says so.
		let r = Requirement::from_json(&fixture("streets.json")).unwrap();

		let source = r.source.as_ref().unwrap();
		assert_eq!(source.schema.as_deref(), Some("shortbread"));
		assert_eq!(source.language.as_deref(), Some("de"));

		let streets = &r.layers["streets"];
		assert_eq!(streets.properties, ["kind", "bridge", "tunnel", "service"]);
		assert_eq!(streets.keep.len(), 2, "keep is a disjunction of entries");

		let first = &streets.keep[0];
		assert_eq!((first.minzoom, first.maxzoom), (Some(5), Some(14)));
		assert!(first.source.is_some(), "the original filter is carried through");
		assert_eq!(
			first.predicate,
			Some(Predicate::And(vec![
				Predicate::In(
					"kind".to_string(),
					vec![
						Literal::String("motorway".to_string()),
						Literal::String("trunk".to_string())
					]
				),
				Predicate::Ne("bridge".to_string(), Literal::Bool(true)),
			]))
		);

		let second = &streets.keep[1];
		assert_eq!((second.minzoom, second.maxzoom), (Some(12), Some(14)));
		assert_eq!(second.predicate, None);

		let buildings = &r.layers["buildings"];
		assert!(buildings.properties.is_empty());
		assert_eq!(buildings.keep.len(), 1);
	}

	#[test]
	fn reads_every_operator_in_the_closed_set() {
		let r = Requirement::from_json(&fixture("operators.json")).unwrap();
		let keep = &r.layers["all"].keep;

		let ops: Vec<&Predicate> = keep.iter().filter_map(|e| e.predicate.as_ref()).collect();
		assert_eq!(ops.len(), 12, "one entry per operator in the closed set");
		assert!(
			!ops.iter().any(|p| matches!(p, Predicate::Unknown)),
			"every documented operator has its own variant: {ops:?}"
		);
	}

	#[test]
	fn an_unknown_operator_parses_rather_than_failing() {
		// The over-approximation contract in action: a future exporter may emit an operator this
		// build has never seen, and refusing the whole file would be worse than keeping too much.
		let json = r#"{"version":1,"layers":{"a":{"keep":[{"where":{"op":"matches","field":"k","re":"^x"}}]}}}"#;
		let r = Requirement::from_json(json).unwrap();
		assert_eq!(r.layers["a"].keep[0].predicate, Some(Predicate::Unknown));
	}

	#[test]
	fn an_unknown_operator_nested_inside_a_known_one_parses_too() {
		// Widening has to survive nesting, or an `and` containing one unknown conjunct would
		// fail the file rather than widen that conjunct.
		let json = r#"{"version":1,"layers":{"a":{"keep":[{"where":
			{"op":"and","args":[{"op":"has","field":"k"},{"op":"sorcery","field":"k"}]}}]}}}"#;
		let r = Requirement::from_json(json).unwrap();
		assert_eq!(
			r.layers["a"].keep[0].predicate,
			Some(Predicate::And(vec![
				Predicate::Has("k".to_string()),
				Predicate::Unknown
			]))
		);
	}

	#[test]
	fn a_malformed_known_operator_is_an_error_not_a_widening() {
		// The widening rule covers operators this build has never heard of, not ones it knows
		// and cannot read. An `eq` with no `value` is a broken exporter, and quietly treating it
		// as "keep everything" would hide the bug behind a tileset that is merely larger than it
		// should be.
		let json = r#"{"version":1,"layers":{"a":{"keep":[{"where":{"op":"eq","field":"k"}}]}}}"#;
		let err = Requirement::from_json(json).unwrap_err();
		assert!(
			err.chain().any(|e| e.to_string().contains("`value` is required")),
			"the error should name the missing field, got: {err:?}"
		);
	}

	#[test]
	fn an_error_names_the_layer_it_came_from() {
		// A requirement has one entry per source-layer, so "`field` is required" without a layer
		// name is a hunt through the file.
		let json = r#"{"version":1,"layers":{"streets":{"keep":[{"where":{"op":"has"}}]}}}"#;
		let err = Requirement::from_json(json).unwrap_err();
		assert!(
			err.chain().any(|e| e.to_string().contains(r#"in layer "streets""#)),
			"expected the layer name in the context chain, got: {err:?}"
		);
	}

	#[test]
	fn rejects_a_version_it_does_not_understand() {
		let err = Requirement::from_json(r#"{"version":2,"layers":{}}"#).unwrap_err();
		assert_eq!(err.to_string(), "unsupported requirement format version 2, expected 1");
	}

	#[test]
	fn rejects_json_that_is_not_a_requirement() {
		assert!(Requirement::from_json("{}").is_err(), "no version, no layers");
		assert!(Requirement::from_json("not json").is_err());
		assert!(Requirement::from_json("[]").is_err(), "not an object");
	}

	#[test]
	fn rejects_a_zoom_that_is_not_a_whole_number_in_range() {
		// `as u8` would turn 300 into 44 — a plausible-looking zoom that filters the wrong tiles.
		for bad in ["300", "14.5", "-1"] {
			let json = format!(r#"{{"version":1,"layers":{{"a":{{"keep":[{{"minzoom":{bad}}}]}}}}}}"#);
			let err = Requirement::from_json(&json).unwrap_err();
			assert!(
				err.chain().any(|e| e.to_string().contains("whole number")),
				"minzoom={bad} should be rejected, got: {err:?}"
			);
		}
	}

	#[test]
	fn literals_keep_their_type() {
		let json = r#"{"version":1,"layers":{"a":{"keep":[
			{"where":{"op":"in","field":"k","values":[1,2.5,"x",true]}}]}}}"#;
		let r = Requirement::from_json(json).unwrap();
		let Some(Predicate::In(_, values)) = &r.layers["a"].keep[0].predicate else {
			panic!("expected an `in` predicate");
		};
		assert_eq!(
			values,
			&[
				Literal::Number(1.0),
				Literal::Number(2.5),
				Literal::String("x".to_string()),
				Literal::Bool(true),
			]
		);
	}

	#[test]
	fn a_number_is_rendered_the_way_it_was_written() {
		// JSON has one number type, so `1000` arrives as `1000.0` and has to be written back
		// without the decimal point — `population >= 1000.0` is not what the style asked for.
		assert_eq!(Literal::render_number(1000.0), "1000");
		assert_eq!(Literal::render_number(-7.0), "-7");
		assert_eq!(Literal::render_number(0.0), "0");
		assert_eq!(Literal::render_number(2.5), "2.5");
		assert_eq!(Literal::render_number(-0.25), "-0.25");

		// Beyond the range where an f64 counts whole numbers, the cast is not available: Rust
		// saturates `f64 as i64`, so an unguarded cast would render 1e300 as i64::MAX — a
		// different number, silently. The range guard is what keeps that from happening.
		let huge = Literal::render_number(1e300);
		assert_ne!(huge, i64::MAX.to_string(), "a huge value must not be clamped");
		assert_eq!(
			huge.parse::<f64>().unwrap().to_bits(),
			1e300_f64.to_bits(),
			"and must still read back as itself"
		);
	}

	#[test]
	fn a_value_that_cannot_be_compared_against_is_rejected() {
		for bad in ["null", "[1]", "{}"] {
			let json = format!(
				r#"{{"version":1,"layers":{{"a":{{"keep":[{{"where":{{"op":"eq","field":"k","value":{bad}}}}}]}}}}}}"#
			);
			assert!(Requirement::from_json(&json).is_err(), "value={bad} should be rejected");
		}
	}

	#[test]
	fn a_layer_may_omit_everything_optional() {
		let r = Requirement::from_json(r#"{"version":1,"layers":{"a":{}}}"#).unwrap();
		let layer = &r.layers["a"];
		assert!(layer.properties.is_empty());
		assert!(
			layer.keep.is_empty(),
			"no keep entry means nothing of this layer is drawn"
		);
	}
}
