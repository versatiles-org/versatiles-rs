//! Which properties a style reads, per source-layer.
//!
//! A style names a property in more ways than one, and missing any of them is not a visible
//! failure — it is a property stripped from the tileset and a label that renders blank months
//! later. So this errs toward naming a property the style does not read: an extra property
//! survives the reduction, which costs bytes, where a missed one costs data.
//!
//! Three spellings are read:
//!
//! 1. **`["get", "kind"]`** — the modern expression form, anywhere in `filter`, `paint` or
//!    `layout`.
//! 2. **A bare string in position 1 of a legacy filter operator** — `["has", "housenumber"]`,
//!    `["==", "kind", "motorway"]`. Nothing in the grammar marks it as a field; it is one because
//!    the operator says so, which is why [`LEGACY_FIELD_OPS`] has to be enumerated.
//! 3. **`{token}` inside a `layout` string** — `"text-field": "{ref}"`. See below.
//!
//! ## Why tokens are read, and why from all of `layout`
//!
//! MapLibre still expands `{property}` inside `text-field` and `icon-image` when the value is a
//! plain string rather than an expression. Every VersaTiles style deployed at the time of writing
//! uses it: `label-motorway-shield` draws `street_labels` with `"text-field": "{ref}"`, and `ref`
//! appears nowhere else in the style — not through `get`, not in a filter. A walk that descends
//! only into arrays never sees it, so `ref` would be stripped from `street_labels` and every
//! motorway shield would render blank. `{housenumber}` is read the same way and escapes only by
//! accident, because its layer happens to also carry `["has", "housenumber"]`.
//!
//! Tokens are only *expanded* in `text-field` and `icon-image`, but every string under `layout` is
//! scanned rather than those two keys alone. The cost of scanning wider is a property kept that
//! need not have been, and no other layout property in practice contains braces at all; the cost
//! of scanning narrower is the failure above, reintroduced the first time a token appears somewhere
//! this file did not predict.

use std::collections::{BTreeMap, BTreeSet};

use versatiles_core::json::{JsonObject, JsonValue};

/// Filter operators that name a field directly, as a bare string in position 1.
///
/// `all` / `any` / `none` are deliberately absent: they take sub-filters, not a field name.
const LEGACY_FIELD_OPS: &[&str] = &["==", "!=", "<", "<=", ">", ">=", "in", "!in", "has", "!has"];

/// Every field an expression reads, by either array spelling.
///
/// Applied to `paint` and `layout` as well as `filter`, where a legacy operator cannot occur. The
/// shape it matches there — an operator string followed by a bare string — is not something the
/// modern grammar produces, since expression operands are themselves expressions or literals
/// wrapped in `["literal", …]`. A false positive only adds a property name, which widens.
fn fields_in(value: &JsonValue, out: &mut BTreeSet<String>) {
	let JsonValue::Array(array) = value else { return };
	let items = array.as_vec();

	if let [JsonValue::String(op), JsonValue::String(name), ..] = items.as_slice() {
		if op == "get" && items.len() == 2 {
			out.insert(name.clone());
		}
		if LEGACY_FIELD_OPS.contains(&op.as_str()) {
			out.insert(name.clone());
		}
	}

	for item in items {
		fields_in(item, out);
	}
}

/// Every `{property}` token in a string, as MapLibre would expand it.
///
/// Empty and nested braces are ignored: `{}` names nothing, and MapLibre does not nest.
fn tokens_in(text: &str, out: &mut BTreeSet<String>) {
	let mut rest = text;
	while let Some(open) = rest.find('{') {
		rest = &rest[open + 1..];
		let Some(close) = rest.find('}') else { return };
		let name = &rest[..close];
		if !name.is_empty() && !name.contains('{') {
			out.insert(name.to_string());
		}
		rest = &rest[close + 1..];
	}
}

/// Every token in every string of a `layout` value, however deeply nested.
fn tokens_of_layout(value: &JsonValue, out: &mut BTreeSet<String>) {
	match value {
		JsonValue::String(text) => tokens_in(text, out),
		JsonValue::Array(array) => {
			for item in array.iter() {
				tokens_of_layout(item, out);
			}
		}
		JsonValue::Object(object) => {
			for (_, item) in object.iter() {
				tokens_of_layout(item, out);
			}
		}
		_ => {}
	}
}

/// The properties one style layer reads.
pub fn fields_of_layer(layer: &JsonObject) -> BTreeSet<String> {
	let mut fields = BTreeSet::new();

	if let Some(filter) = layer.get("filter") {
		fields_in(filter, &mut fields);
	}
	for key in ["paint", "layout"] {
		if let Some(JsonValue::Object(object)) = layer.get(key) {
			for (_, value) in object.iter() {
				fields_in(value, &mut fields);
			}
		}
	}
	// Only `layout` carries tokens — `paint` has no string-valued property MapLibre expands.
	if let Some(layout) = layer.get("layout") {
		tokens_of_layout(layout, &mut fields);
	}

	fields
}

/// Source-layer → the properties the style reads from it.
///
/// Style layers with no `source-layer` draw no tile data and are skipped.
pub fn usage(layers: &[&JsonObject]) -> BTreeMap<String, BTreeSet<String>> {
	let mut used: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

	for layer in layers {
		let Ok(Some(source_layer)) = layer.string("source-layer") else {
			continue;
		};
		used.entry(source_layer).or_default().extend(fields_of_layer(layer));
	}

	used
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	fn layer(json: &str) -> JsonObject {
		JsonObject::parse_str(json).unwrap()
	}

	fn fields(json: &str) -> Vec<String> {
		fields_of_layer(&layer(json)).into_iter().collect()
	}

	#[test]
	fn reads_the_modern_get_form() {
		assert_eq!(fields(r#"{"filter":["==",["get","kind"],"motorway"]}"#), ["kind"]);
	}

	#[test]
	fn reads_the_legacy_bare_string_form() {
		// `["has","housenumber"]` names a field with nothing in the grammar marking it as one.
		assert_eq!(fields(r#"{"filter":["has","housenumber"]}"#), ["housenumber"]);
	}

	#[test]
	fn reads_fields_from_paint_and_layout() {
		assert_eq!(
			fields(r#"{"paint":{"fill-color":["get","colour"]},"layout":{"text-field":["get","name"]}}"#),
			["colour", "name"]
		);
	}

	#[test]
	fn reads_a_token_text_field() {
		// The regression this module exists for: `{ref}` is the only mention of `ref` in every
		// deployed VersaTiles style, and an array-only walk never sees it.
		assert_eq!(fields(r#"{"layout":{"text-field":"{ref}"}}"#), ["ref"]);
	}

	#[test]
	fn reads_several_tokens_from_one_string() {
		assert_eq!(fields(r#"{"layout":{"text-field":"{ref} — {name}"}}"#), ["name", "ref"]);
	}

	#[test]
	fn an_empty_or_unclosed_token_names_nothing() {
		assert_eq!(fields(r#"{"layout":{"text-field":"{} {unclosed"}}"#), Vec::<String>::new());
	}

	#[test]
	fn paint_strings_are_not_scanned_for_tokens() {
		// MapLibre expands no token in paint, and a colour like `#fff` carries no braces anyway —
		// but a literal string that happens to look like one must not invent a property.
		assert_eq!(fields(r#"{"paint":{"fill-color":"{not-a-field}"}}"#), Vec::<String>::new());
	}

	#[test]
	fn nested_expressions_are_walked() {
		assert_eq!(
			fields(r#"{"filter":["all",["==",["get","a"],1],["any",["has","b"],["!",["has","c"]]]]}"#),
			["a", "b", "c"]
		);
	}

	#[test]
	fn a_literal_list_is_not_read_as_a_field() {
		// `["literal",["motorway","trunk"]]` holds values, not field names.
		assert_eq!(
			fields(r#"{"filter":["in",["get","kind"],["literal",["motorway","trunk"]]]}"#),
			["kind"]
		);
	}

	#[test]
	fn usage_merges_layers_sharing_a_source_layer() {
		let a = layer(r#"{"source-layer":"streets","filter":["has","kind"]}"#);
		let b = layer(r#"{"source-layer":"streets","layout":{"text-field":"{ref}"}}"#);
		let c = layer(r##"{"paint":{"background-color":"#fff"}}"##);
		let used = usage(&[&a, &b, &c]);

		assert_eq!(used.len(), 1, "the layer without a source-layer draws no data");
		assert_eq!(
			used["streets"].iter().cloned().collect::<Vec<_>>(),
			["kind", "ref"]
		);
	}
}
