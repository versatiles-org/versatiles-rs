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
//! from the pyramid of the source it wraps, which is the only place that knows it. When the style's
//! source declares a lower `maxzoom`, that one wins: MapLibre never requests a tile above it and
//! overzooms the tiles at it instead, so those are the tiles that must carry the feature.
//!
//! A tile zoom is the map zoom rounded down — MapLibre floors it for vector sources — so a layer
//! drawn from map zoom 12.5 is drawn from z12 tiles, and one hidden from 10.5 still draws z10 tiles.
//!
//! So each tile keeps what the style draws at that tile's zoom. A view pitched beyond about 60°,
//! one with terrain, and the globe projection load several tile zooms at once — lower toward the
//! horizon — and get from each tile the detail of its zoom, as they would from any tileset built
//! with per-zoom detail.

mod fields;
mod filter;

use std::{
	borrow::Cow,
	collections::{BTreeMap, BTreeSet, HashMap},
	ops::RangeInclusive,
};

use anyhow::{Context, Result, bail, ensure};
use versatiles_core::json::{JsonObject, JsonValue};

use super::ir::{KeepEntry, LayerRequirement, Predicate, Properties, Requirement};

/// The zoom range one group of style layers is drawn in, already clamped to the tileset.
type ZoomRange = (u8, u8);

/// Reads a zoom from a style object, as the tile zoom it falls in: rounded down.
fn tile_zoom(object: &JsonObject, key: &str) -> Result<Option<u8>> {
	let Some(value) = object.number(key)? else {
		return Ok(None);
	};
	ensure!(
		(0.0..=30.0).contains(&value),
		"{key} must be a number between 0 and 30, found {value}"
	);
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "the ensure! above admits only numbers in 0..=30, and `floor` makes them whole"
	)]
	Ok(Some(value.floor() as u8))
}

/// Reads a style layer's zoom bounds as tile zooms, clamped to what the tiles actually carry.
///
/// Both are rounded down, because that is the tile zoom MapLibre loads at a map zoom: a layer
/// drawn from 12.5 is drawn from z12 tiles. A style layer's `maxzoom` is **exclusive** in MapLibre
/// — the layer is hidden at zooms greater than or equal to it — and is read here as inclusive. For a
/// fractional one that is exact (hidden from 10.5, still drawn at 10.4, from z10 tiles); for a whole
/// one it keeps one zoom level more than the style draws, which is the widening direction.
fn zoom_range(layer: &JsonObject, maxzoom: u8) -> Result<ZoomRange> {
	let read = |key: &str| tile_zoom(layer, key);

	let min = read("minzoom")?.unwrap_or(0).min(maxzoom);
	let max = read("maxzoom")?.unwrap_or(maxzoom).min(maxzoom);

	Ok((min, max))
}

/// How a style source appears in messages: its ID, or what it is when the layers name none.
fn source_name(source: Option<&String>) -> String {
	source.map_or_else(|| "layers without a `source`".to_string(), |id| format!("{id:?}"))
}

/// What identifies the tileset a style source reads: its TileJSON `url`, or its list of `tiles`
/// URLs. A source with neither, or with no definition in `sources`, is only itself.
fn tileset_identity(style: &JsonObject, source: Option<&String>) -> String {
	if let Some(definition) = source_definition(style, source.map(String::as_str)) {
		if let Some(JsonValue::String(url)) = definition.get("url") {
			return format!("url {url}");
		}
		if let Some(tiles @ JsonValue::Array(_)) = definition.get("tiles") {
			return format!("tiles {}", tiles.stringify());
		}
	}
	format!("source {source:?}")
}

/// The style sources that read one tileset, with what their layers draw.
#[derive(Default)]
struct Tileset {
	/// The IDs of the sources, `None` for layers that name none.
	ids: BTreeSet<Option<String>>,
	/// The source-layers their layers draw.
	drawn: BTreeSet<String>,
}

impl Tileset {
	/// How it appears in messages: its source ID, or all of them when several read it.
	fn name(&self) -> String {
		self
			.ids
			.iter()
			.map(|id| source_name(id.as_ref()))
			.collect::<Vec<_>>()
			.join(" and ")
	}
}

/// What a `ref` layer takes from the layer it names, as MapLibre's `refProperties` lists it.
const REF_PROPERTIES: &[&str] = &[
	"type",
	"source",
	"source-layer",
	"minzoom",
	"maxzoom",
	"filter",
	"layout",
];

/// The style layers with every `ref` resolved, the way MapLibre resolves them when it loads a style.
///
/// A `ref` layer, from before style expressions, takes [`REF_PROPERTIES`] from the layer it names
/// and has only its own `paint`. MapLibre GL JS still accepts one (`derefLayers` in `style.ts`).
/// Unresolved, it is a layer without a source-layer that draws nothing, and the properties its
/// `paint` reads would be stripped. A `ref` naming no layer is left as it is; MapLibre refuses
/// such a style anyway.
fn resolve_refs<'a>(layers: &[&'a JsonObject]) -> Result<Vec<Cow<'a, JsonObject>>> {
	let by_id: HashMap<String, &JsonObject> = layers
		.iter()
		.filter_map(|layer| Some((layer.string("id").ok()??, *layer)))
		.collect();

	layers
		.iter()
		.map(|layer| {
			let Some(parent) = layer.string("ref")?.and_then(|id| by_id.get(&id)) else {
				return Ok(Cow::Borrowed(*layer));
			};
			let mut resolved = JsonObject::new();
			for (key, value) in layer.iter().filter(|(key, _)| *key != "ref") {
				resolved.set(key, value);
			}
			for key in REF_PROPERTIES {
				if let Some(value) = parent.get(key) {
					resolved.set(key, value);
				}
			}
			Ok(Cow::Owned(resolved))
		})
		.collect()
}

/// Whether a style layer is hidden: `"visibility": "none"` in its `layout`.
///
/// A hidden layer draws nothing, so it keeps nothing. A map that shows it at runtime with
/// `setLayoutProperty` needs a style with it visible for the reduction. `visibility` cannot be
/// data-driven, so the plain string is all there is to read.
fn is_hidden(layer: &JsonObject) -> bool {
	matches!(
		layer.get("layout"),
		Some(JsonValue::Object(layout))
			if matches!(layout.get("visibility"), Some(JsonValue::String(visibility)) if visibility == "none")
	)
}

/// The style layers that draw the tileset being reduced.
///
/// Hidden layers ([`is_hidden`]) are left out before anything else: they draw nothing, so they
/// keep nothing, and do not count as a tileset the style draws either.
///
/// A style may draw several vector tilesets, and only one of them is this one. Counting the
/// others' layers would merge their filters into this tileset's, and a style whose data layers all
/// belong to another tileset would reduce this one to nothing.
///
/// The candidates are read from the layers rather than from the style's `sources`: only a vector
/// source has source-layers, so a layer that names one is drawing a vector source, and a source no
/// data layer draws from does not matter here. Sources with the same `url` or the same `tiles` are
/// one tileset — a style may read it twice, under one ID for the map and another for its labels —
/// and are selected together.
///
/// `sources` picks by ID; naming one source of a tileset selects all of them. Without it, a style
/// drawing a single tileset uses that one, and a style drawing several is refused: the source IDs
/// are whatever the style's author chose, so nothing in them says which is this tileset. The
/// tileset's own layer names can say which one *matches*, and the message suggests it, but two
/// Shortbread-like tilesets share their names, so it stays a hint.
///
/// `tileset_layers` is empty when the tileset does not list its layers; the checks against it are
/// then skipped.
fn select_layers<'a>(
	style: &JsonObject,
	layers: &[&'a JsonObject],
	sources: &[String],
	tileset_layers: &BTreeSet<String>,
) -> Result<Vec<&'a JsonObject>> {
	// Every style layer that draws tile data, with the tileset its source reads.
	let mut data_layers: Vec<(&JsonObject, String)> = Vec::new();
	let mut tilesets: BTreeMap<String, Tileset> = BTreeMap::new();
	let mut any_hidden = false;
	for layer in layers {
		let Some(source_layer) = layer.string("source-layer")? else {
			continue;
		};
		if is_hidden(layer) {
			any_hidden = true;
			continue;
		}
		let id = layer.string("source")?;
		let identity = tileset_identity(style, id.as_ref());
		let tileset = tilesets.entry(identity.clone()).or_default();
		tileset.ids.insert(id);
		tileset.drawn.insert(source_layer);
		data_layers.push((layer, identity));
	}

	ensure!(
		!tilesets.is_empty(),
		"the style draws no source-layer, so reducing a tileset to it would leave nothing. \
		 Check that this is a built style with data layers rather than, say, an empty or \
		 background-only one.{}",
		if any_hidden {
			" Its data layers are all hidden (`\"visibility\": \"none\"`), and a hidden layer draws nothing.".to_string()
		} else {
			String::new()
		}
	);

	let list = || tilesets.values().map(Tileset::name).collect::<Vec<_>>().join(", ");

	let selected: BTreeSet<&String> = if sources.is_empty() {
		if tilesets.len() > 1 {
			let matching: Vec<&Tileset> = tilesets
				.values()
				.filter(|tileset| !tileset.drawn.is_disjoint(tileset_layers))
				.collect();
			let hint = match matching.as_slice() {
				[tileset] => format!(" This tileset's layers match source {}.", tileset.name()),
				_ => String::new(),
			};
			bail!(
				"the style draws {} tilesets ({}), and nothing in it says which one is this tileset. \
				 Set `source` to the ID of the one to reduce to.{hint}",
				tilesets.len(),
				list()
			);
		}
		tilesets.keys().collect()
	} else {
		sources
			.iter()
			.map(|id| {
				tilesets
					.iter()
					.find(|(_, tileset)| tileset.ids.contains(&Some(id.clone())))
					.map(|(identity, _)| identity)
					.with_context(|| {
						format!(
							"no style layer draws source-layers from source {id:?}. The style draws them from: {}",
							list()
						)
					})
			})
			.collect::<Result<_>>()?
	};

	let drawn: BTreeSet<&String> = selected
		.iter()
		.flat_map(|identity| &tilesets[*identity].drawn)
		.collect();
	ensure!(
		tileset_layers.is_empty() || drawn.iter().any(|layer| tileset_layers.contains(*layer)),
		"the style draws none of this tileset's layers, so reducing it would leave nothing. \
		 The style draws: {}. The tileset has: {}.",
		drawn.iter().map(|layer| layer.as_str()).collect::<Vec<_>>().join(", "),
		tileset_layers.iter().cloned().collect::<Vec<_>>().join(", ")
	);

	// In the style's order: it is the order the merged filters are written in.
	Ok(data_layers
		.into_iter()
		.filter(|(_, identity)| selected.contains(identity))
		.map(|(layer, _)| layer)
		.collect())
}

/// The definition of a style source in the style's `sources`, if it has one.
fn source_definition<'a>(style: &'a JsonObject, source: Option<&str>) -> Option<&'a JsonObject> {
	let Some(JsonValue::Object(sources)) = style.get("sources") else {
		return None;
	};
	match sources.get(source?)? {
		JsonValue::Object(definition) => Some(definition),
		_ => None,
	}
}

/// The property a style source promotes to the feature ID, per source-layer.
///
/// MapLibre reads it for `feature-state` (hover, selection) and for `["id"]`, without the style
/// ever naming it in a `get`, so it has to be kept on top of what the layers read. `promoteId` is
/// either one name for every source-layer or an object naming one per source-layer.
fn promoted_id(style: &JsonObject, source: Option<&str>, source_layer: &str) -> Option<String> {
	match source_definition(style, source)?.get("promoteId")? {
		JsonValue::String(name) => Some(name.clone()),
		JsonValue::Object(per_layer) => match per_layer.get(source_layer)? {
			JsonValue::String(name) => Some(name.clone()),
			_ => None,
		},
		_ => None,
	}
}

/// Where a style layer's source stops: the tileset's maximum, or the source's own `maxzoom` if that
/// is lower.
fn source_maxzoom(style: &JsonObject, layer: &JsonObject, tileset_maxzoom: u8) -> Result<u8> {
	let id = layer.string("source")?;
	Ok(match source_definition(style, id.as_deref()) {
		Some(definition) => tile_zoom(definition, "maxzoom")
			.with_context(|| format!("in style source {}", source_name(id.as_ref())))?
			.map_or(tileset_maxzoom, |source_max| source_max.min(tileset_maxzoom)),
		None => tileset_maxzoom,
	})
}

/// The promoted IDs of every source whose layers draw `source_layer`; see [`promoted_id`].
fn promoted_ids(style: &JsonObject, layers: &[&JsonObject], source_layer: &str) -> Result<BTreeSet<String>> {
	let mut ids = BTreeSet::new();
	for layer in layers {
		if layer.string("source-layer")?.as_deref() == Some(source_layer) {
			ids.extend(promoted_id(style, layer.string("source")?.as_deref(), source_layer));
		}
	}
	Ok(ids)
}

/// One keep entry per zoom range of a source-layer, from the filters of the style layers drawn in it.
///
/// `maxzoom` is where the selection's tiles stop; a bound reaching it is left out.
fn keep_entries(ranges: BTreeMap<ZoomRange, Vec<Option<&JsonValue>>>, maxzoom: u8) -> Vec<KeepEntry> {
	ranges
		.into_iter()
		.map(|((min, max), filters)| {
			let mut predicates = Vec::new();
			// One style layer that keeps everything absorbs the rest of the range: the disjunction
			// is already satisfied for every feature, so narrowing it with the others' predicates
			// would drop features this range draws.
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
					// Simplified, not merely built. One style layer per drawn case is how cartography
					// is written, so this disjunction is the whole of a source-layer's styling
					// flattened into one predicate — 232 terms for `colorful`'s `streets`. `simplify`
					// is exact; what it removes is repetition, not meaning.
					_ => Some(Predicate::Or(predicates)),
				}
				.map(crate::reduce::simplify::simplify)
			};

			KeepEntry {
				// A bound that covers the whole pyramid is not worth emitting: it would render as
				// `zoom >= 0`, which every tile satisfies, and cost a CEL evaluation to say so.
				minzoom: (min > 0).then_some(min),
				maxzoom: (max < maxzoom).then_some(max),
				predicate,
			}
		})
		.collect()
}

/// What a style needs from a tileset whose deepest level is `maxzoom`.
///
/// Only the style layers drawing one tileset count. `sources` names it by the IDs of its style
/// sources, empty to let the style say; sources with the same URL are one tileset. Without it, a
/// style drawing a single tileset uses that one. `tileset_layers` are the layer names the tileset
/// lists, empty if it lists none; they suggest a source when the style draws several tilesets, and
/// refuse a style that draws none of them.
///
/// Style layers that share a source-layer **and** a clamped zoom range are merged into one entry
/// with their predicates OR-ed together. That is semantically identical to listing them separately
/// — `keep` is a disjunction either way — and much smaller: the deployed `colorful` style has 318
/// filters across 324 layers, and merging turns them into a few entries per source-layer.
///
/// # Errors
///
/// Fails if the JSON is not a style object, if it draws no source-layer at all, if it draws several
/// tilesets and `sources` does not pick one, or if what it draws is none of the tileset's layers. The
/// last three are the one place where failing beats widening: each would reduce a tileset to
/// nothing, and "you pointed me at the wrong file" is a far likelier reading than "delete
/// everything".
pub fn from_style(
	json: &str,
	maxzoom: u8,
	sources: &[String],
	tileset_layers: &BTreeSet<String>,
) -> Result<Requirement> {
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
	let resolved = resolve_refs(&layers)?;
	let layers: Vec<&JsonObject> = resolved.iter().map(AsRef::as_ref).collect();
	let layers = select_layers(style, &layers, sources, tileset_layers)?;

	// Several sources reading one tileset may stop at different zooms, so the clamp is per layer,
	// and the deepest of them is where the selection as a whole stops.
	let tileset_maxzoom = maxzoom;
	let mut maxzoom = 0;
	for layer in &layers {
		maxzoom = maxzoom.max(source_maxzoom(style, layer, tileset_maxzoom)?);
	}

	// source-layer → clamped zoom range → the filters of the style layers contributing to it. A
	// `None` filter is a style layer that draws everything, which is why the option is kept rather
	// than skipped: it is what makes the whole range keep everything.
	let mut grouped: BTreeMap<String, BTreeMap<ZoomRange, Vec<Option<&JsonValue>>>> = BTreeMap::new();
	// Every style layer with the tile zooms it is drawn at, for the properties it reads there.
	let mut drawn_at: Vec<(&JsonObject, RangeInclusive<u8>)> = Vec::new();

	for layer in &layers {
		let Some(source_layer) = layer.string("source-layer")? else {
			continue;
		};
		let range = zoom_range(layer, source_maxzoom(style, layer, tileset_maxzoom)?).with_context(|| {
			format!(
				"in style layer {:?}",
				layer.string("id").ok().flatten().unwrap_or_default()
			)
		})?;

		drawn_at.push((layer, range.0..=range.1));
		grouped
			.entry(source_layer)
			.or_default()
			.entry(range)
			.or_default()
			.push(layer.get("filter"));
	}

	let usage = fields::usage(&drawn_at);
	let mut requirement_layers = BTreeMap::new();

	for (source_layer, ranges) in grouped {
		let keep = keep_entries(ranges, maxzoom);
		let mut usage = usage.get(&source_layer).cloned().unwrap_or_default();

		// Every property a predicate reads must survive the reduction, or the filter that needs it
		// evaluates `has(props.k)` against a feature that no longer carries it and drops the whole
		// layer. `fields::usage` already walks filters, so this should be a no-op — but "should be"
		// is what the `{ref}` token also was, and the cost of being wrong here is silent data loss
		// while the cost of the union is nothing. The feature filter runs before properties are
		// stripped, so the range only has to cover the entry's own zooms.
		for entry in &keep {
			if let Some(predicate) = &entry.predicate {
				let mut names = BTreeSet::new();
				filter::fields(predicate, &mut names);
				let zooms = entry.minzoom.unwrap_or(0)..=entry.maxzoom.unwrap_or(maxzoom);
				for name in names {
					usage.add(name, &zooms);
				}
			}
		}

		// The promoted ID identifies a feature at every zoom it is drawn at.
		for name in promoted_ids(style, &layers, &source_layer)? {
			usage.add(name, &(0..=maxzoom));
		}

		requirement_layers.insert(
			source_layer,
			LayerRequirement {
				properties: if usage.all {
					Properties::All
				} else {
					Properties::Listed(usage.names)
				},
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
		from_style(json, maxzoom, &[], &BTreeSet::new()).unwrap()
	}

	/// The listed properties of a layer, with their zooms; a layer keeping all of them fails the
	/// test.
	fn listed(properties: &Properties) -> &BTreeMap<String, RangeInclusive<u8>> {
		match properties {
			Properties::Listed(names) => names,
			Properties::All => panic!("expected a list of properties, found all of them"),
		}
	}

	fn properties(requirement: &Requirement, layer: &str) -> Vec<String> {
		listed(&requirement.layers[layer].properties).keys().cloned().collect()
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
			.flat_map(|layer| listed(&layer.properties).keys())
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
			let kept: BTreeSet<String> = listed(&layer.properties).keys().cloned().collect();
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
		let error = from_style(&style("empty.json"), 14, &[], &BTreeSet::new()).unwrap_err();
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
	fn a_fractional_zoom_is_the_tile_zoom_it_falls_in() {
		// Drawn from map zoom 12.5 means drawn from z12 tiles; hidden from 10.5 means still drawn
		// at 10.4, from z10 tiles.
		let requirement = from(
			r#"{"layers":[{"id":"a","source-layer":"water","minzoom":12.5,"maxzoom":13.5}]}"#,
			14,
		);
		let entry = &requirement.layers["water"].keep[0];
		assert_eq!((entry.minzoom, entry.maxzoom), (Some(12), Some(13)));
	}

	#[test]
	fn a_lower_source_maxzoom_is_where_the_tiles_stop() {
		// MapLibre requests nothing above the source's maxzoom and overzooms z12 instead, so a layer
		// drawn from 13 must be kept in the z12 tiles, although the tileset goes on to z14.
		let requirement = from(
			r#"{"sources":{"s":{"type":"vector","maxzoom":12}},"layers":[
				{"id":"a","source":"s","source-layer":"water","minzoom":13}
			]}"#,
			14,
		);
		let entry = &requirement.layers["water"].keep[0];
		assert_eq!((entry.minzoom, entry.maxzoom), (Some(12), None));
	}

	#[test]
	fn a_higher_source_maxzoom_does_not_raise_the_tilesets() {
		let requirement = from(
			r#"{"sources":{"s":{"type":"vector","maxzoom":18}},"layers":[
				{"id":"a","source":"s","source-layer":"addresses","minzoom":17}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers["addresses"].keep[0].minzoom, Some(14));
	}

	#[test]
	fn a_broken_source_maxzoom_names_the_source() {
		let error = from_style(
			r#"{"sources":{"s":{"type":"vector","maxzoom":"high"}},"layers":[
				{"id":"a","source":"s","source-layer":"water"}
			]}"#,
			14,
			&[],
			&BTreeSet::new(),
		)
		.unwrap_err();
		assert!(format!("{error:#}").contains(r#"style source "s""#), "got: {error:#}");
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

	// ── sources ──

	/// A style drawing `water` from both `shortbread` and `overlay`, and `ocean` from `overlay` only.
	const TWO_SOURCES: &str = r#"{"layers":[
		{"id":"bg","type":"background"},
		{"id":"hills","type":"raster","source":"hillshade"},
		{"id":"a","source":"shortbread","source-layer":"water","filter":["==","kind","lake"]},
		{"id":"b","source":"overlay","source-layer":"water","filter":["==","kind","river"]},
		{"id":"c","source":"overlay","source-layer":"ocean"}
	]}"#;

	fn layers(names: &[&str]) -> BTreeSet<String> {
		names.iter().map(ToString::to_string).collect()
	}

	fn ids(sources: &[&str]) -> Vec<String> {
		sources.iter().map(ToString::to_string).collect()
	}

	fn error(json: &str, sources: &[&str], tileset: &[&str]) -> String {
		format!(
			"{:#}",
			from_style(json, 14, &ids(sources), &layers(tileset)).unwrap_err()
		)
	}

	#[test]
	fn the_only_vector_source_is_used_without_being_named() {
		// The raster and background layers draw no source-layer, so they are no second candidate.
		let requirement = from_style(
			r#"{"layers":[
				{"id":"hills","type":"raster","source":"hillshade"},
				{"id":"a","source":"shortbread","source-layer":"water"}
			]}"#,
			14,
			&[],
			&layers(&["water"]),
		)
		.unwrap();
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["water"]);
	}

	#[test]
	fn several_sources_are_refused_without_naming_one() {
		let message = error(TWO_SOURCES, &[], &[]);
		assert!(
			message.contains(r#"2 tilesets ("overlay", "shortbread")"#),
			"got: {message}"
		);
		assert!(message.contains("Set `source`"), "got: {message}");
		assert!(!message.contains("match"), "no tileset layers, so no hint: {message}");
	}

	#[test]
	fn the_refusal_suggests_the_source_the_tileset_matches() {
		// `ocean` is drawn from `overlay` alone, so a tileset with `ocean` points there.
		let message = error(TWO_SOURCES, &[], &["ocean", "land"]);
		assert!(message.contains(r#"match source "overlay""#), "got: {message}");
	}

	#[test]
	fn the_refusal_suggests_nothing_when_the_tileset_matches_both() {
		// Both sources draw `water`: the names cannot tell them apart, so no guess is offered.
		let message = error(TWO_SOURCES, &[], &["water"]);
		assert!(!message.contains("match source"), "got: {message}");
	}

	/// A style reading one tileset twice — `map` and `labels` share a URL — and an `overlay`.
	const ONE_TILESET_TWICE: &str = r#"{
		"sources":{
			"map":{"type":"vector","url":"https://example.org/tiles.json"},
			"labels":{"type":"vector","url":"https://example.org/tiles.json","maxzoom":12},
			"overlay":{"type":"vector","url":"https://example.org/overlay.json"}
		},
		"layers":[
			{"id":"a","source":"map","source-layer":"water","filter":["==","kind","lake"]},
			{"id":"b","source":"labels","source-layer":"places","minzoom":13,"layout":{"text-field":["get","name"]}},
			{"id":"c","source":"overlay","source-layer":"water","filter":["==","kind","river"]}
		]
	}"#;

	#[test]
	fn sources_sharing_a_url_are_one_tileset() {
		let without_overlay = ONE_TILESET_TWICE.replace(
			r#",
			{"id":"c","source":"overlay","source-layer":"water","filter":["==","kind","river"]}"#,
			"",
		);
		let requirement = from_style(&without_overlay, 14, &[], &BTreeSet::new()).unwrap();
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["places", "water"]);
	}

	#[test]
	fn sources_sharing_their_tiles_are_one_tileset() {
		let requirement = from(
			r#"{"sources":{
				"a":{"type":"vector","tiles":["https://example.org/{z}/{x}/{y}"]},
				"b":{"type":"vector","tiles":["https://example.org/{z}/{x}/{y}"]}
			},"layers":[
				{"id":"a","source":"a","source-layer":"water"},
				{"id":"b","source":"b","source-layer":"places"}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["places", "water"]);
	}

	#[test]
	fn naming_one_source_of_a_tileset_selects_all_of_them() {
		let requirement = from_style(ONE_TILESET_TWICE, 14, &ids(&["map"]), &BTreeSet::new()).unwrap();
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["places", "water"]);
		// Only `map`'s lakes, not `overlay`'s rivers.
		assert_eq!(
			requirement.layers["water"].keep[0].predicate,
			Some(Predicate::Eq(
				"kind".to_string(),
				crate::reduce::Literal::String("lake".to_string())
			))
		);
	}

	#[test]
	fn each_source_stops_at_its_own_maxzoom() {
		// `labels` stops at z12, so its z13 layer is kept in z12 tiles; `map` goes on to the
		// tileset's z14.
		let requirement = from_style(ONE_TILESET_TWICE, 14, &ids(&["map"]), &BTreeSet::new()).unwrap();
		let places = &requirement.layers["places"].keep[0];
		assert_eq!((places.minzoom, places.maxzoom), (Some(12), Some(12)));
		let water = &requirement.layers["water"].keep[0];
		assert_eq!((water.minzoom, water.maxzoom), (None, None));
	}

	#[test]
	fn the_refusal_names_the_sources_of_one_tileset_together() {
		let message = error(ONE_TILESET_TWICE, &[], &[]);
		assert!(
			message.contains(r#"2 tilesets ("overlay", "labels" and "map")"#),
			"got: {message}"
		);
	}

	#[test]
	fn a_list_can_select_two_tilesets() {
		// For tilesets that are the same although their URLs differ.
		let requirement = from_style(TWO_SOURCES, 14, &ids(&["shortbread", "overlay"]), &BTreeSet::new()).unwrap();
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["ocean", "water"]);
	}

	#[test]
	fn a_named_source_counts_only_its_own_layers() {
		// Without the selection, `water` would keep lakes *or* rivers; `shortbread` draws lakes only.
		let requirement = from_style(TWO_SOURCES, 14, &ids(&["shortbread"]), &layers(&["water"])).unwrap();
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["water"]);
		assert_eq!(
			requirement.layers["water"].keep[0].predicate,
			Some(Predicate::Eq(
				"kind".to_string(),
				crate::reduce::Literal::String("lake".to_string())
			))
		);
	}

	#[test]
	fn an_unknown_source_is_refused_with_the_ones_there_are() {
		let message = error(TWO_SOURCES, &["osm"], &[]);
		assert!(message.contains(r#"source "osm""#), "got: {message}");
		assert!(message.contains(r#""overlay", "shortbread""#), "got: {message}");
	}

	#[test]
	fn a_source_that_draws_no_source_layer_is_refused() {
		// `hillshade` is in the style, but as a raster: it is not a tileset this could reduce.
		let message = error(TWO_SOURCES, &["hillshade"], &[]);
		assert!(message.contains(r#"source "hillshade""#), "got: {message}");
	}

	#[test]
	fn a_style_drawing_none_of_the_tilesets_layers_is_refused() {
		// One source, so nothing to choose — but it is not this tileset, and every tile would be
		// emptied.
		let message = error(
			r#"{"layers":[{"id":"a","source":"s","source-layer":"water"}]}"#,
			&[],
			&["streets", "buildings"],
		);
		assert!(message.contains("none of this tileset's layers"), "got: {message}");
		assert!(message.contains("draws: water"), "got: {message}");
		assert!(message.contains("has: buildings, streets"), "got: {message}");
	}

	// ── ref layers ──

	#[test]
	fn a_ref_layer_keeps_what_its_paint_reads() {
		// `b` draws `water` through `a`, and reads `depth`, which `a` does not.
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"water","paint":{"c":["get","kind"]}},
				{"id":"b","ref":"a","paint":{"o":["get","depth"]}}
			]}"#,
			14,
		);
		assert_eq!(properties(&requirement, "water"), ["depth", "kind"]);
	}

	#[test]
	fn a_ref_layer_takes_its_parents_filter_and_zooms() {
		// `b`'s own `filter` and `minzoom` are ignored, as MapLibre ignores them.
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"water","minzoom":10,"filter":["==","kind","lake"],"paint":{"c":["get","name"]}},
				{"id":"b","ref":"a","minzoom":3,"filter":["has","x"],"paint":{"o":["get","depth"]}}
			]}"#,
			14,
		);
		let keep = &requirement.layers["water"].keep;
		assert_eq!(keep.len(), 1, "both layers share a zoom range: {keep:?}");
		assert_eq!(keep[0].minzoom, Some(10));
		assert_eq!(listed(&requirement.layers["water"].properties)["depth"], 10..=14);
	}

	#[test]
	fn a_ref_layer_of_a_hidden_layer_is_hidden() {
		// `layout`, and with it `visibility`, comes from the parent.
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"water","layout":{"visibility":"none"}},
				{"id":"b","ref":"a","paint":{"o":["get","depth"]}},
				{"id":"c","source-layer":"land"}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["land"]);
	}

	#[test]
	fn a_ref_naming_no_layer_draws_nothing() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"water"},
				{"id":"b","ref":"missing","paint":{"o":["get","depth"]}}
			]}"#,
			14,
		);
		assert_eq!(properties(&requirement, "water"), Vec::<String>::new());
	}

	// ── hidden layers ──

	#[test]
	fn a_hidden_layer_keeps_nothing() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"water","paint":{"c":["get","kind"]}},
				{"id":"b","source-layer":"water","layout":{"visibility":"none"},"paint":{"c":["get","name"]}},
				{"id":"c","source-layer":"pois","layout":{"visibility":"none"}},
				{"id":"d","source-layer":"land","layout":{"visibility":"visible"}}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["land", "water"]);
		assert_eq!(properties(&requirement, "water"), ["kind"]);
	}

	#[test]
	fn a_hidden_layer_does_not_widen_the_filter() {
		// The visible layer draws only lakes; the hidden one, unfiltered, would keep everything.
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"water","filter":["==","kind","lake"]},
				{"id":"b","source-layer":"water","layout":{"visibility":"none"}}
			]}"#,
			14,
		);
		assert!(requirement.layers["water"].keep[0].predicate.is_some());
	}

	#[test]
	fn a_hidden_layer_of_another_tileset_needs_no_source() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source":"map","source-layer":"water"},
				{"id":"b","source":"overlay","source-layer":"routes","layout":{"visibility":"none"}}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers.keys().collect::<Vec<_>>(), ["water"]);
	}

	#[test]
	fn a_style_whose_data_layers_are_all_hidden_is_refused() {
		let message = error(
			r#"{"layers":[{"id":"a","source-layer":"water","layout":{"visibility":"none"}}]}"#,
			&[],
			&[],
		);
		assert!(message.contains("draws no source-layer"), "got: {message}");
		assert!(message.contains("data layers are all hidden"), "got: {message}");
	}

	// ── properties per zoom ──

	#[test]
	fn a_property_is_kept_from_the_lowest_zoom_a_layer_reads_it() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"streets","minzoom":5,"paint":{"c":["get","kind"]}},
				{"id":"b","source-layer":"streets","minzoom":12,"layout":{"text-field":"{ref}"},"paint":{"c":["get","kind"]}},
				{"id":"c","source-layer":"streets","maxzoom":8,"paint":{"c":["get","old"]}}
			]}"#,
			14,
		);
		let zooms = listed(&requirement.layers["streets"].properties);
		assert_eq!(zooms["kind"], 5..=14);
		assert_eq!(zooms["ref"], 12..=14);
		assert_eq!(zooms["old"], 0..=8);
	}

	#[test]
	fn a_promoted_id_is_kept_at_every_zoom() {
		let requirement = from(
			r#"{"sources":{"s":{"type":"vector","promoteId":"osm_id"}},"layers":[
				{"id":"a","source":"s","source-layer":"water","minzoom":10}
			]}"#,
			14,
		);
		assert_eq!(listed(&requirement.layers["water"].properties)["osm_id"], 0..=14);
	}

	// ── properties that cannot be listed ──

	#[test]
	fn a_computed_property_name_keeps_every_property_of_that_layer() {
		let requirement = from(
			r#"{"layers":[
				{"id":"a","source-layer":"places","layout":{"text-field":["get",["concat","name_","de"]]}},
				{"id":"b","source-layer":"water","paint":{"c":["get","kind"]}}
			]}"#,
			14,
		);
		assert_eq!(requirement.layers["places"].properties, Properties::All);
		assert_eq!(
			properties(&requirement, "water"),
			["kind"],
			"other layers are unaffected"
		);
	}

	#[test]
	fn a_promoted_id_is_kept() {
		// `feature-state` needs it, and no `get` names it.
		let requirement = from(
			r#"{"sources":{"s":{"type":"vector","promoteId":"osm_id"}},"layers":[
				{"id":"a","source":"s","source-layer":"water","paint":{"c":["get","kind"]}},
				{"id":"b","source":"s","source-layer":"land"}
			]}"#,
			14,
		);
		assert_eq!(properties(&requirement, "water"), ["kind", "osm_id"]);
		assert_eq!(properties(&requirement, "land"), ["osm_id"]);
	}

	#[test]
	fn a_promoted_id_per_source_layer_is_kept_where_it_applies() {
		let requirement = from(
			r#"{"sources":{"s":{"type":"vector","promoteId":{"water":"water_id"}}},"layers":[
				{"id":"a","source":"s","source-layer":"water"},
				{"id":"b","source":"s","source-layer":"land"}
			]}"#,
			14,
		);
		assert_eq!(properties(&requirement, "water"), ["water_id"]);
		assert_eq!(properties(&requirement, "land"), Vec::<String>::new());
	}

	#[test]
	fn another_sources_promoted_id_is_not_kept() {
		let requirement = from_style(
			r#"{"sources":{"s":{"type":"vector"},"t":{"type":"vector","promoteId":"other_id"}},"layers":[
				{"id":"a","source":"s","source-layer":"water"},
				{"id":"b","source":"t","source-layer":"water"}
			]}"#,
			14,
			&ids(&["s"]),
			&BTreeSet::new(),
		)
		.unwrap();
		assert_eq!(properties(&requirement, "water"), Vec::<String>::new());
	}

	// ── malformed input ──

	#[test]
	fn a_non_object_style_is_refused() {
		assert!(
			from_style("[]", 14, &[], &BTreeSet::new())
				.unwrap_err()
				.to_string()
				.contains("not a JSON object")
		);
		assert!(
			from_style("{", 14, &[], &BTreeSet::new())
				.unwrap_err()
				.to_string()
				.contains("not valid JSON")
		);
	}

	#[test]
	fn a_style_without_layers_is_refused() {
		assert!(
			from_style("{}", 14, &[], &BTreeSet::new())
				.unwrap_err()
				.to_string()
				.contains("no `layers`")
		);
	}

	#[test]
	fn a_broken_zoom_names_the_style_layer() {
		let error = from_style(
			r#"{"layers":[{"id":"oops","source-layer":"w","minzoom":-1}]}"#,
			14,
			&[],
			&BTreeSet::new(),
		)
		.unwrap_err();
		assert!(error.chain().any(|e| e.to_string().contains("oops")), "got: {error:?}");
	}
}
