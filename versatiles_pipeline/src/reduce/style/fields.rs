//! Which properties a style reads, per source-layer.
//!
//! A style names a property in more ways than one, and missing any of them is not a visible
//! failure — it is a property stripped from the tileset and a label that renders blank months
//! later. So this errs toward naming a property the style does not read: an extra property
//! survives the reduction, which costs bytes, where a missed one costs data.
//!
//! Four spellings are read:
//!
//! 1. **`["get", "kind"]`** — the modern expression form, anywhere in `filter`, `paint` or
//!    `layout`.
//! 2. **A bare string in position 1 of a legacy filter operator** — `["has", "housenumber"]`,
//!    `["==", "kind", "motorway"]`. Nothing in the grammar marks it as a field; it is one because
//!    the operator says so, which is why [`LEGACY_FIELD_OPS`] has to be enumerated.
//! 3. **`{token}` inside a `layout` string** — `"text-field": "{ref}"`. See below.
//! 4. **The `property` of a legacy function** — `{"property": "kind", "stops": […]}`, the
//!    data-driven styling that predates expressions.
//!
//! Some expressions read properties whose names cannot be listed: `["get", ["concat", "name_",
//! lang]]` computes the name, and `["properties"]` hands over all of them. A layer using either
//! keeps **every** property ([`Fields::all`]), because any name left off the list would be one the
//! style might read.
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

use std::{
	collections::{BTreeMap, BTreeSet},
	ops::RangeInclusive,
};

use versatiles_core::json::{JsonObject, JsonValue};

/// Filter operators that name a field directly, as a bare string in position 1.
///
/// `all` / `any` / `none` are deliberately absent: they take sub-filters, not a field name.
const LEGACY_FIELD_OPS: &[&str] = &["==", "!=", "<", "<=", ">", ">=", "in", "!in", "has", "!has"];

/// The properties a style layer, or a source-layer's style layers together, read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fields {
	/// The properties named.
	pub names: BTreeSet<String>,
	/// Whether properties are also read by a name that cannot be listed, so that all of them have
	/// to stay.
	pub all: bool,
}

/// Every field an expression reads, by any spelling.
///
/// Applied to `paint` and `layout` as well as `filter`, where a legacy operator cannot occur. The
/// shape it matches there — an operator string followed by a bare string — is not something the
/// modern grammar produces, since expression operands are themselves expressions or literals
/// wrapped in `["literal", …]`. A false positive only adds a property name, which widens.
///
/// Objects are walked too. A legacy function is one, and expressions nest inside objects as well:
/// the options of `["format", …, {"text-color": ["get", "colour"]}]` are expressions in their own
/// right. Walking the occasional `["literal", {…}]` as well only finds names that need not stay.
fn fields_in(value: &JsonValue, out: &mut Fields) {
	match value {
		JsonValue::Array(array) => {
			let items = array.as_vec();

			match items.as_slice() {
				// The whole property map, to a name chosen at runtime or to anything at all.
				[JsonValue::String(op)] if op == "properties" => out.all = true,
				// `["get", "k"]` and modern `["has", "k"]` name it. A three-argument form reads from
				// the object in its third argument instead; when that is `["properties"]`, the walk
				// below finds it.
				[JsonValue::String(op), name, rest @ ..] if op == "get" || op == "has" => match name {
					JsonValue::String(name) if rest.is_empty() => {
						out.names.insert(name.clone());
					}
					JsonValue::String(_) => {}
					// A name computed at runtime: any property may be the one it reads.
					_ => out.all = true,
				},
				_ => {}
			}

			if let [JsonValue::String(op), JsonValue::String(name), ..] = items.as_slice()
				// `$type` and `$id` in a legacy filter are the geometry type and the feature ID,
				// which no tile stores as a property.
				&& LEGACY_FIELD_OPS.contains(&op.as_str())
				&& !super::filter::LEGACY_SPECIAL_KEYS.contains(&name.as_str())
			{
				out.names.insert(name.clone());
			}

			for item in items {
				fields_in(item, out);
			}
		}
		JsonValue::Object(object) => {
			// A legacy function: `{"property": "kind", "type": "categorical", "stops": […]}`.
			if let Some(JsonValue::String(name)) = object.get("property") {
				out.names.insert(name.clone());
			}
			for (_, item) in object.iter() {
				fields_in(item, out);
			}
		}
		_ => {}
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
pub fn fields_of_layer(layer: &JsonObject) -> Fields {
	let mut fields = Fields::default();

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
		tokens_of_layout(layout, &mut fields.names);
	}

	fields
}

/// What a source-layer's style layers read, and at which tile zooms.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
	/// Each property read, with the zooms of the style layers that read it.
	pub names: BTreeMap<String, RangeInclusive<u8>>,
	/// Whether a property is read by a name that cannot be listed, so that all of them have to
	/// stay at every zoom.
	pub all: bool,
}

impl Usage {
	/// Records `name` as read at `zooms`, widening the range it is already read at.
	pub fn add(&mut self, name: String, zooms: &RangeInclusive<u8>) {
		self
			.names
			.entry(name)
			.and_modify(|range| {
				*range = (*range.start()).min(*zooms.start())..=(*range.end()).max(*zooms.end());
			})
			.or_insert_with(|| zooms.clone());
	}
}

/// Source-layer → the properties the style reads from it, and at which zooms.
///
/// Each style layer comes with the tile zooms it is drawn at; style layers with no `source-layer`
/// draw no tile data and are skipped.
pub fn usage(layers: &[(&JsonObject, RangeInclusive<u8>)]) -> BTreeMap<String, Usage> {
	let mut used: BTreeMap<String, Usage> = BTreeMap::new();

	for (layer, zooms) in layers {
		let Ok(Some(source_layer)) = layer.string("source-layer") else {
			continue;
		};
		let fields = fields_of_layer(layer);
		let usage = used.entry(source_layer).or_default();
		usage.all |= fields.all;
		for name in fields.names {
			usage.add(name, zooms);
		}
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
		let fields = fields_of_layer(&layer(json));
		assert!(!fields.all, "expected a list of names: {json}");
		fields.names.into_iter().collect()
	}

	fn reads_all(json: &str) -> bool {
		fields_of_layer(&layer(json)).all
	}

	#[test]
	fn reads_the_modern_get_form() {
		assert_eq!(fields(r#"{"filter":["==",["get","kind"],"motorway"]}"#), ["kind"]);
	}

	#[test]
	fn legacy_special_keys_are_not_properties() {
		assert_eq!(
			fields(r#"{"filter":["all",["==","$type","Polygon"],["in","$id",1],["has","kind"]]}"#),
			["kind"]
		);
		// The modern `get` names a real property, whatever it is called.
		assert_eq!(fields(r#"{"paint":{"c":["get","$type"]}}"#), ["$type"]);
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
		assert_eq!(
			fields(r#"{"layout":{"text-field":"{} {unclosed"}}"#),
			Vec::<String>::new()
		);
	}

	#[test]
	fn paint_strings_are_not_scanned_for_tokens() {
		// MapLibre expands no token in paint, and a colour like `#fff` carries no braces anyway —
		// but a literal string that happens to look like one must not invent a property.
		assert_eq!(
			fields(r#"{"paint":{"fill-color":"{not-a-field}"}}"#),
			Vec::<String>::new()
		);
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
		let used = usage(&[(&a, 0..=14), (&b, 0..=14), (&c, 0..=14)]);

		assert_eq!(used.len(), 1, "the layer without a source-layer draws no data");
		assert_eq!(
			used["streets"].names.keys().cloned().collect::<Vec<_>>(),
			["kind", "ref"]
		);
	}

	#[test]
	fn a_property_is_read_at_the_zooms_of_the_layers_reading_it() {
		let a = layer(r#"{"source-layer":"streets","filter":["has","kind"]}"#);
		let b = layer(r#"{"source-layer":"streets","layout":{"text-field":"{ref}"},"paint":{"c":["get","kind"]}}"#);
		let c = layer(r#"{"source-layer":"streets","paint":{"c":["get","ref"]}}"#);
		let used = usage(&[(&a, 5..=8), (&b, 12..=14), (&c, 10..=11)]);

		assert_eq!(used["streets"].names["kind"], 5..=14, "spans both layers reading it");
		assert_eq!(used["streets"].names["ref"], 10..=14);
	}

	#[test]
	fn a_computed_property_name_keeps_them_all() {
		assert!(reads_all(
			r#"{"layout":{"text-field":["get",["concat","name_","de"]]}}"#
		));
		assert!(reads_all(r#"{"filter":["has",["get","which"]]}"#));
	}

	#[test]
	fn the_whole_property_map_keeps_them_all() {
		assert!(reads_all(r#"{"layout":{"text-field":["get","name",["properties"]]}}"#));
		assert!(reads_all(r#"{"paint":{"c":["to-string",["properties"]]}}"#));
	}

	#[test]
	fn a_get_from_a_literal_object_reads_no_property() {
		// The third argument is the object read from, and here it is not the feature's.
		assert_eq!(
			fields(r##"{"paint":{"c":["get","k",["literal",{"k":"#fff"}]]}}"##),
			Vec::<String>::new()
		);
	}

	#[test]
	fn a_listable_layer_does_not_keep_them_all() {
		assert!(!reads_all(
			r#"{"filter":["has","kind"],"layout":{"text-field":["get","name"]}}"#
		));
	}

	#[test]
	fn reads_the_property_of_a_legacy_function() {
		assert_eq!(
			fields(r##"{"paint":{"fill-color":{"property":"kind","type":"categorical","stops":[["a","#f00"]]}}}"##),
			["kind"]
		);
		assert_eq!(
			fields(r#"{"layout":{"icon-image":{"property":"icon","type":"identity"}}}"#),
			["icon"]
		);
	}

	#[test]
	fn reads_expressions_inside_format_options() {
		assert_eq!(
			fields(r#"{"layout":{"text-field":["format",["get","name"],{"text-color":["get","colour"]}]}}"#),
			["colour", "name"]
		);
	}

	#[test]
	fn usage_keeps_them_all_if_one_layer_does() {
		let a = layer(r#"{"source-layer":"streets","filter":["has","kind"]}"#);
		let b = layer(r#"{"source-layer":"streets","layout":{"text-field":["get",["concat","name_","de"]]}}"#);
		let used = usage(&[(&a, 0..=14), (&b, 0..=14)]);
		assert!(used["streets"].all);
		assert!(used["streets"].names.contains_key("kind"));
	}
}
