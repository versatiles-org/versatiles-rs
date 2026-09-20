//! What a MapLibre style needs from a tileset.
//!
//! Reads a built style and answers, per source-layer: which properties are read, and which features
//! are drawn at which zooms. That answer is a [`Requirement`], which the renderer turns into the
//! filter operations that strip everything else away.
//!
//! Pure: JSON in, [`Requirement`] out. No tiles, no I/O, no network — which is what makes it
//! testable as golden text and callable from a pipeline operation's `build`.
//!
//! ## The three rules
//!
//! 1. **`keep` is an OR.** A feature is needed if any entry matches within its zoom range.
//! 2. **An entry with no predicate keeps everything in that zoom range.**
//! 3. **The whole answer is a conservative over-approximation.** It may keep features the style
//!    never draws; it must never drop one it does. Anything that cannot be read widens to rule 2.
//!
//! ## Zooms are tile zooms, and that is not cosmetic
//!
//! Every zoom here is clamped to the tileset's own maximum, and getting this wrong is the one way
//! this can silently destroy data. MapLibre overzooms: `addresses` is drawn from style zoom 17, but
//! in a Shortbread tileset that data exists *only* in z14 tiles. A requirement of "from z17"
//! applied to a z14 tile drops every address in the tileset. So the style's z17 is emitted as z14 —
//! the zoom at which the tiles must actually carry the feature.
//!
//! That is why [`from_style`] insists on being told the tileset maximum. The operation reads it
//! from the pyramid of the source it wraps, which is the only place that knows it.

mod fields;
mod filter;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use versatiles_core::json::{JsonObject, JsonValue};

use super::ir::{KeepEntry, LayerRequirement, Predicate, Requirement};

/// The zoom range one group of style layers is drawn in, already clamped to the tileset.
type ZoomRange = (u8, u8);

/// Reads a style layer's zoom bounds, clamped to what the tiles actually carry.
///
/// A style layer's `maxzoom` is **exclusive** in MapLibre — the layer is hidden at zooms greater
/// than or equal to it — and is read here as inclusive. That keeps one zoom level more than the
/// style draws, which is the widening direction, and it avoids an off-by-one that would drop the
/// deepest level of every bounded layer.
fn zoom_range(layer: &JsonObject, maxzoom: u8) -> Result<ZoomRange> {
	let read = |key: &str| -> Result<Option<u8>> {
		let Some(value) = layer.number(key)? else {
			return Ok(None);
		};
		ensure!(
			value.fract() == 0.0 && (0.0..=30.0).contains(&value),
			"{key} must be a whole number between 0 and 30, found {value}"
		);
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "the ensure! above admits only whole numbers in 0..=30"
		)]
		Ok(Some(value as u8))
	};

	let min = read("minzoom")?.unwrap_or(0).min(maxzoom);
	let max = read("maxzoom")?.unwrap_or(maxzoom).min(maxzoom);

	Ok((min, max))
}

/// What a style needs from a tileset whose deepest level is `maxzoom`.
///
/// Style layers that share a source-layer **and** a clamped zoom range are merged into one entry
/// with their predicates OR-ed together. That is semantically identical to listing them separately
/// — `keep` is a disjunction either way — and much smaller: the deployed `colorful` style has 318
/// filters across 324 layers, and merging turns them into a few entries per source-layer.
///
/// # Errors
///
/// Fails if the JSON is not a style object, or if it draws no source-layer at all. The second is
/// the one place where failing beats widening: a style that names no tile data reduces a tileset to
/// nothing, and "you pointed me at the wrong file" is a far likelier reading than "delete
/// everything".
pub fn from_style(json: &str, maxzoom: u8) -> Result<Requirement> {
	let value = JsonValue::parse_str(json).context("style is not valid JSON")?;
	let style = value.as_object().context("style is not a JSON object")?;

	let layers: Vec<&JsonObject> = match style.get("layers") {
		Some(JsonValue::Array(array)) => array
			.iter()
			.map(|item| item.as_object().context("every style layer must be an object"))
			.collect::<Result<_>>()?,
		Some(_) => bail!("the style's `layers` is not an array"),
		None => bail!("the style has no `layers`"),
	};

	let usage = fields::usage(&layers);

	// source-layer → clamped zoom range → the filters of the style layers contributing to it. A
	// `None` filter is a style layer that draws everything, which is why the option is kept rather
	// than skipped: it is what makes the whole range keep everything.
	let mut grouped: BTreeMap<String, BTreeMap<ZoomRange, Vec<Option<&JsonValue>>>> = BTreeMap::new();

	for layer in &layers {
		let Some(source_layer) = layer.string("source-layer")? else {
			continue;
		};
		let range = zoom_range(layer, maxzoom).with_context(|| {
			format!(
				"in style layer {:?}",
				layer.string("id").ok().flatten().unwrap_or_default()
			)
		})?;

		grouped
			.entry(source_layer)
			.or_default()
			.entry(range)
			.or_default()
			.push(layer.get("filter"));
	}

	ensure!(
		!grouped.is_empty(),
		"the style draws no source-layer, so reducing a tileset to it would leave nothing. \
		 Check that this is a built style with data layers rather than, say, an empty or \
		 background-only one."
	);

	let mut requirement_layers = BTreeMap::new();

	for (source_layer, ranges) in grouped {
		let keep: Vec<KeepEntry> = ranges
			.into_iter()
			.map(|((min, max), filters)| {
				let mut predicates = Vec::new();
				// One style layer that keeps everything absorbs the rest of the range: the
				// disjunction is already satisfied for every feature, so narrowing it with the
				// others' predicates would drop features this range draws.
				let mut keeps_everything = false;

				for filter in filters {
					match filter.map(filter::normalize) {
						// No filter at all, or one that could not be read.
						None | Some(None) => keeps_everything = true,
						Some(Some(predicate)) => predicates.push(predicate),
					}
				}

				let predicate = if keeps_everything {
					None
				} else {
					match predicates.len() {
						0 => None,
						1 => predicates.into_iter().next(),
						// Simplified, not merely built. One style layer per drawn case is how
						// cartography is written, so this disjunction is the whole of a source-layer's
						// styling flattened into one predicate — 232 terms for `colorful`'s `streets`.
						// `simplify` is exact; what it removes is repetition, not meaning.
						_ => Some(Predicate::Or(predicates)),
					}
					.map(crate::reduce::simplify::simplify)
				};

				KeepEntry {
					// A bound that covers the whole pyramid is not worth emitting: it would render
					// as `zoom >= 0`, which every tile satisfies, and cost a CEL evaluation to say
					// so.
					minzoom: (min > 0).then_some(min),
					maxzoom: (max < maxzoom).then_some(max),
					predicate,
				}
			})
			.collect();

		let mut properties = usage.get(&source_layer).cloned().unwrap_or_default();

		// Every property a predicate reads must survive the reduction, or the filter that needs it
		// evaluates `has(props.k)` against a feature that no longer carries it and drops the whole
		// layer. `fields::usage` already walks filters, so this should be a no-op — but "should be"
		// is what the `{ref}` token also was, and the cost of being wrong here is silent data loss
		// while the cost of the union is nothing.
		for entry in &keep {
			if let Some(predicate) = &entry.predicate {
				filter::fields(predicate, &mut properties);
			}
		}

		requirement_layers.insert(
			source_layer,
			LayerRequirement {
				properties: properties.into_iter().collect(),
				keep,
			},
		);
	}

	Ok(Requirement {
		layers: requirement_layers,
	})
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use std::collections::BTreeSet;

	use pretty_assertions::assert_eq;

	use super::*;

	/// Every property a requirement's predicates read.
	///
	/// A predicate that reads a property the reduction strips would be evaluated against a feature
	/// that no longer carries it, and `has(props.k)` would answer `false` for every one of them.
	fn predicate_fields(requirement: &Requirement, layer: &str) -> BTreeSet<String> {
		let mut out = BTreeSet::new();
		if let Some(requirements) = requirement.layers.get(layer) {
			for entry in &requirements.keep {
				if let Some(predicate) = &entry.predicate {
					filter::fields(predicate, &mut out);
				}
			}
		}
		out
	}

	fn style(path: &str) -> String {
		let path = format!("{}/../testdata/styles/{path}", env!("CARGO_MANIFEST_DIR"));
		std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
	}

	fn from(json: &str, maxzoom: u8) -> Requirement {
		from_style(json, maxzoom).unwrap()
	}

	fn properties(requirement: &Requirement, layer: &str) -> Vec<String> {
		requirement.layers[layer].properties.clone()
	}

	// ── the vendored styles ──

	#[test]
	fn colorful_names_the_shortbread_layers() {
		let requirement = from(&style("colorful.json"), 14);
		assert_eq!(requirement.layers.len(), 21);
		for layer in ["street_labels", "addresses", "buildings", "water_polygons"] {
			assert!(requirement.layers.contains_key(layer), "missing {layer}");
		}
	}

	#[test]
	fn a_token_text_field_keeps_its_property() {
		// `label-motorway-shield` draws `street_labels` with `"text-field": "{ref}"`, and `ref`
		// appears nowhere else in the style — not through `get`, not in a filter. A walk that
		// descends only into arrays never sees it, and the reduction would strip `ref` from
		// `street_labels`, rendering every motorway shield blank.
		assert!(
			properties(&from(&style("colorful.json"), 14), "street_labels").contains(&"ref".to_string()),
			"ref survives"
		);
		assert!(
			properties(&from(&style("neutrino.json"), 14), "street_labels").contains(&"ref".to_string()),
			"and in neutrino, which uses the same token"
		);
	}

	#[test]
	fn a_token_icon_field_keeps_its_property() {
		assert!(properties(&from(&style("colorful.json"), 14), "addresses").contains(&"housenumber".to_string()));
	}

	#[test]
	fn no_translation_is_read_by_the_deployed_styles() {
		// The point of the whole exercise: these styles read `name` and no `name_*` at all, so
		// every translation in a Shortbread tileset can go.
		let requirement = from(&style("colorful.json"), 14);
		let translations: Vec<String> = requirement
			.layers
			.values()
			.flat_map(|layer| layer.properties.iter())
			.filter(|property| property.starts_with("name_") || property.starts_with("name:"))
			.cloned()
			.collect();
		assert_eq!(translations, Vec::<String>::new());
	}

	#[test]
	fn every_predicate_field_survives_the_reduction() {
		// A `where` that reads a stripped property would evaluate `has(props.k)` against a feature
		// that no longer carries it, and drop every feature of the layer.
		let requirement = from(&style("colorful.json"), 14);
		for (name, layer) in &requirement.layers {
			let read = predicate_fields(&requirement, name);
			let kept: BTreeSet<String> = layer.properties.iter().cloned().collect();
			assert!(
				read.is_subset(&kept),
				"{name}: predicates read {:?} which the reduction would strip",
				read.difference(&kept).collect::<Vec<_>>()
			);
		}
	}

	#[test]
	fn a_style_that_draws_no_data_is_refused() {
		// `empty.json` has no layers at all. Reducing to it would produce an empty tileset, and
		// "wrong file" is a likelier reading than "delete everything".
		let error = from_style(&style("empty.json"), 14).unwrap_err();
		assert!(error.to_string().contains("draws no source-layer"), "got: {error}");
	}

	// ── zoom handling ──

	#[test]
	fn zooms_are_clamped_to_the_tileset() {
		// The `addresses` case: drawn from style z17, but the data only exists in z14 tiles, so an
		// unclamped `zoom >= 17` would drop every address.
		let requirement = from(
			r#"{"layers":[{"id":"a","source-layer":"addresses","minzoom":17,"filter":["has","housenumber"]}]}"#,
			14,
		);
		assert_eq!(requirement.layers["addresses"].keep[0].minzoom, Some(14));
	}

	#[test]
	fn a_full_range_emits_no_zoom_bound() {
		// `zoom >= 0` is satisfied by every tile and costs an evaluation to say so.
		let requirement = from(
			r#"{"layers":[{"id":"a","source-layer":"water","minzoom":0,"maxzoom":14}]}"#,
			14,
		);
		let entry = &requirement.layers["water"].keep[0];
		assert_eq!((entry.minzoom, entry.maxzoom), (None, None));
	}

	#[test]
	fn a_style_maxzoom_is_read_as_inclusive() {
		// MapLibre's layer `maxzoom` is exclusive. Reading it as inclusive keeps one zoom more than
		// the style draws, which is the widening direction; reading it exactly would drop the
		// deepest level of every bounded layer.
		let requirement = from(r#"{"layers":[{"id":"a","source-layer":"water","maxzoom":10}]}"#, 14);
		assert_eq!(requirement.layers["water"].keep[0].maxzoom, Some(10));
	}

	#[test]
	fn layers_sharing_a_range_are_merged_into_one_disjunction() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"streets","minzoom":5,"filter":["==","kind","motorway"]},
				{"id":"b","source-layer":"streets","minzoom":5,"filter":["==","kind","trunk"]},
				{"id":"c","source-layer":"streets","minzoom":9,"filter":["has","service"]}
			]}"#,
			14,
		);
		let keep = &requirement.layers["streets"].keep;
		assert_eq!(keep.len(), 2, "two zoom ranges, not three layers");
		// The two z5 layers are OR-ed, and `simplify` then recognises that a disjunction of
		// equalities on one field is a membership test. One layer per drawn value is how
		// cartography is written, so this is the shape almost every merged range takes.
		assert_eq!(
			keep[0].predicate,
			Some(Predicate::In(
				"kind".to_string(),
				vec![
					crate::reduce::Literal::String("motorway".to_string()),
					crate::reduce::Literal::String("trunk".to_string()),
				]
			))
		);
		assert_eq!(keep[0].minzoom, Some(5));
		assert_eq!(keep[1].minzoom, Some(9));
	}

	#[test]
	fn one_unfiltered_layer_makes_its_whole_range_keep_everything() {
		// The disjunction is already satisfied for every feature, so narrowing it with the other
		// layer's predicate would drop features this range draws.
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"streets","minzoom":5,"filter":["==","kind","motorway"]},
				{"id":"b","source-layer":"streets","minzoom":5}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers["streets"].keep[0].predicate, None);
	}

	#[test]
	fn an_unreadable_filter_widens_the_same_way() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"streets","filter":["==","kind","motorway"]},
				{"id":"b","source-layer":"streets","filter":["sorcery","kind"]}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers["streets"].keep[0].predicate, None);
	}

	#[test]
	fn a_layer_without_a_source_layer_draws_no_data() {
		let requirement = from(
			r#"{"layers":[
				{"id":"bg","type":"background"},
				{"id":"a","source-layer":"water"}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["water"]);
	}

	// ── malformed input ──

	#[test]
	fn a_non_object_style_is_refused() {
		assert!(
			from_style("[]", 14)
				.unwrap_err()
				.to_string()
				.contains("not a JSON object")
		);
		assert!(from_style("{", 14).unwrap_err().to_string().contains("not valid JSON"));
	}

	#[test]
	fn a_style_without_layers_is_refused() {
		assert!(from_style("{}", 14).unwrap_err().to_string().contains("no `layers`"));
	}

	#[test]
	fn a_broken_zoom_names_the_style_layer() {
		let error = from_style(r#"{"layers":[{"id":"oops","source-layer":"w","minzoom":14.5}]}"#, 14).unwrap_err();
		assert!(error.chain().any(|e| e.to_string().contains("oops")), "got: {error:?}");
	}
}
