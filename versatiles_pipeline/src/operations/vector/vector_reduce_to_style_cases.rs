//! The reduction's behaviour, case by case, from `tests/reduce_cases/*.json`.
//!
//! The format is described in that directory's `README.md`. Each case states what MapLibre draws
//! (`drawn`) and what the reduction keeps (`kept`), and this checks three things about every one:
//! the output holds everything drawn — the contract —, the output is exactly `kept`, and a `kept`
//! larger than `drawn` declares why (`widening`). Every failure is collected and reported at the
//! end, so one run shows them all.

use std::{
	collections::{BTreeMap, BTreeSet},
	fmt::Write,
	path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use geo_types::{Geometry, LineString, Point, Polygon};
use serde_json::{Map, Value};
use versatiles_core::{TileCoord, TileJSON};
use versatiles_geometry::{
	geo::{GeoFeature, GeoProperties, GeoValue},
	vector_tile::{VectorTile, VectorTileLayer},
};

use super::Runner;
use crate::{operations::transform::VectorTransform, reduce};

/// Why the reduction may keep more than the style draws. A case's `widening.kind` must be one of
/// these; extend the list when a case needs a new reason.
const WIDENING_KINDS: &[&str] = &[
	"unreadable-filter",
	"unreadable-negation",
	"to-boolean",
	"draws-nothing",
	"geometry-type",
	"property-per-source-layer",
	"property-zoom-span",
	"computed-property-name",
	"generous-field-detection",
	"inclusive-maxzoom",
	"zoom-clamp",
];

/// What a tile holds, as far as a case cares: layer → feature ID → property names.
type Content = BTreeMap<String, BTreeMap<u64, BTreeSet<String>>>;

/// One case, parsed and checked for form.
struct Case {
	name: String,
	style: String,
	source: Vec<String>,
	tileset_maxzoom: u8,
	tileset_layers: Option<BTreeSet<String>>,
	fields: Option<BTreeMap<String, BTreeSet<String>>>,
	expect: Expect,
}

/// What a case expects: a refused build, or tiles.
enum Expect {
	Error(String),
	Tiles {
		tile: VectorTile,
		/// Zoom → what MapLibre draws, and what the reduction keeps.
		zooms: BTreeMap<u8, (Content, Content)>,
		widening: Option<String>,
	},
}

#[test]
fn every_case_holds() {
	let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reduce_cases");
	let mut files: Vec<_> = std::fs::read_dir(&dir)
		.unwrap()
		.map(|entry| entry.unwrap().path())
		.filter(|path| path.extension().is_some_and(|e| e == "json"))
		.collect();
	files.sort();
	assert!(!files.is_empty(), "no cases in {}", dir.display());

	let mut failures = Vec::new();
	let mut names = BTreeSet::new();
	let mut exact = 0;
	let mut refused = 0;
	let mut widened: BTreeMap<String, usize> = BTreeMap::new();

	for file in &files {
		let label = file.file_name().unwrap().to_string_lossy().to_string();
		let text = std::fs::read_to_string(file).unwrap();
		let cases: Vec<Value> = match serde_json::from_str(&text) {
			Ok(Value::Array(cases)) => cases,
			Ok(_) => {
				failures.push(format!("{label}: not an array of cases"));
				continue;
			}
			Err(error) => {
				failures.push(format!("{label}: not valid JSON: {error}"));
				continue;
			}
		};

		for (index, value) in cases.iter().enumerate() {
			let case = match parse_case(value) {
				Ok(case) => case,
				Err(error) => {
					failures.push(format!("{label} #{index}: {error:#}"));
					continue;
				}
			};
			let where_ = format!("{label}: {:?}", case.name);
			if !names.insert(case.name.clone()) {
				failures.push(format!("{where_}: the name is used twice"));
			}
			match check(&case) {
				Ok(problems) => failures.extend(problems.into_iter().map(|p| format!("{where_}: {p}"))),
				Err(error) => failures.push(format!("{where_}: {error:#}")),
			}
			match &case.expect {
				Expect::Error(_) => refused += 1,
				Expect::Tiles {
					widening: Some(kind), ..
				} => *widened.entry(kind.clone()).or_default() += 1,
				Expect::Tiles { widening: None, .. } => exact += 1,
			}
		}
	}

	let mut summary = format!("{} cases: {exact} exact, {refused} refused", names.len());
	for (kind, count) in &widened {
		let _ = write!(summary, ", {count} {kind}");
	}
	println!("{summary}");

	assert!(
		failures.is_empty(),
		"{} of the reduction cases failed:\n\n{}",
		failures.len(),
		failures.join("\n\n")
	);
}

/// Runs one case and returns what is wrong with it; an `Err` is a case that could not be run.
fn check(case: &Case) -> Result<Vec<String>> {
	let tileset_layers = match (&case.tileset_layers, &case.expect) {
		(Some(layers), _) => layers.clone(),
		(None, Expect::Tiles { tile, .. }) => tile.layers.iter().map(|l| l.name.clone()).collect(),
		(None, Expect::Error(_)) => BTreeSet::new(),
	};
	let built = reduce::from_style(&case.style, case.tileset_maxzoom, &case.source, &tileset_layers)
		.and_then(|requirement| Runner::from_requirement(&requirement));

	let (tile, runner, zooms, widening) = match (&case.expect, built) {
		(Expect::Error(expected), Err(error)) => {
			let message = format!("{error:#}");
			return Ok(if message.contains(expected.as_str()) {
				Vec::new()
			} else {
				vec![format!("fails with {message:?}, which does not contain {expected:?}")]
			});
		}
		(Expect::Error(expected), Ok(_)) => return Ok(vec![format!("builds, but should fail with {expected:?}")]),
		(Expect::Tiles { .. }, Err(error)) => return Ok(vec![format!("does not build: {error:#}")]),
		(Expect::Tiles { tile, zooms, widening }, Ok(runner)) => (tile, runner, zooms, widening),
	};

	let mut problems = Vec::new();
	let mut any_widening = false;

	for (zoom, (drawn, kept)) in zooms {
		let at = if zooms.len() > 1 {
			format!(" at z{zoom}")
		} else {
			String::new()
		};

		// The case file has to be consistent before the code is judged against it.
		let undrawn = missing(kept, drawn);
		if !undrawn.is_empty() {
			problems.push(format!(
				"`kept`{at} lacks what `drawn` has, which the contract forbids — fix the case: {}",
				undrawn.join("; ")
			));
		}
		any_widening |= kept != drawn;

		let output = match runner.run(&TileCoord::new(*zoom, 0, 0)?, tile.clone())? {
			Some(tile) => content(&tile)?,
			None => Content::new(),
		};

		let dropped = missing(&output, drawn);
		if !dropped.is_empty() {
			problems.push(format!(
				"drops{at} what the style draws — a bug in the reduction: {}",
				dropped.join("; ")
			));
		}
		if &output != kept {
			let less = missing(&output, kept);
			let more = missing(kept, &output);
			let mut message = format!("keeps{at} something other than `kept`.");
			if !less.is_empty() {
				let _ = write!(message, " Less: {}.", less.join("; "));
				if dropped.is_empty() {
					message.push_str(
						" Everything drawn is still kept, so this is an improvement: update `kept`, and drop `widening` if `kept` now equals `drawn`.",
					);
				}
			}
			if !more.is_empty() {
				let _ = write!(
					message,
					" More: {}. A regression, or a new widening to declare.",
					more.join("; ")
				);
			}
			problems.push(message);
		}
	}

	match (any_widening, widening) {
		(true, None) => problems.push("`kept` is larger than `drawn`, so the case needs a `widening`".to_string()),
		(false, Some(kind)) => problems.push(format!("`kept` equals `drawn`, so the `widening` ({kind}) is not one")),
		_ => {}
	}

	if let Some(expected) = &case.fields {
		let actual = tilejson_fields(tile, &runner)?;
		if &actual != expected {
			problems.push(format!("the TileJSON fields are {actual:?}, not {expected:?}"));
		}
	}

	Ok(problems)
}

/// The `vector_layers` fields after the reduction, starting from a TileJSON that lists every layer
/// of `tile` with every property its features carry.
fn tilejson_fields(tile: &VectorTile, runner: &Runner) -> Result<BTreeMap<String, BTreeSet<String>>> {
	let mut tilejson = TileJSON::default();
	for layer in &tile.layers {
		let fields = layer
			.to_features()?
			.iter()
			.flat_map(|feature| {
				feature
					.properties
					.iter()
					.map(|(key, _)| (key.clone(), "String".to_string()))
			})
			.collect();
		tilejson.vector_layers.0.insert(
			layer.name.clone(),
			versatiles_core::VectorLayer {
				fields,
				description: None,
				minzoom: None,
				maxzoom: None,
			},
		);
	}
	runner.update_tilejson(&mut tilejson);
	Ok(tilejson
		.vector_layers
		.iter()
		.map(|(name, layer)| (name.clone(), layer.fields.keys().cloned().collect()))
		.collect())
}

/// What `small` has that `big` lacks, one entry per layer, feature or property.
fn missing(big: &Content, small: &Content) -> Vec<String> {
	let mut out = Vec::new();
	for (layer, features) in small {
		let Some(big_features) = big.get(layer) else {
			out.push(format!("layer `{layer}`"));
			continue;
		};
		for (id, properties) in features {
			let Some(big_properties) = big_features.get(id) else {
				out.push(format!("feature {id} of `{layer}`"));
				continue;
			};
			for property in properties.difference(big_properties) {
				out.push(format!("`{property}` of feature {id} of `{layer}`"));
			}
		}
	}
	out
}

/// What a tile holds: every layer's feature IDs and their property names.
fn content(tile: &VectorTile) -> Result<Content> {
	let mut out = Content::new();
	for layer in &tile.layers {
		let features = out.entry(layer.name.clone()).or_default();
		for feature in layer.to_features()? {
			let id = feature.id.as_ref().context("a feature lost its ID")?.as_u64()?;
			features.insert(id, feature.properties.iter().map(|(key, _)| key.clone()).collect());
		}
	}
	Ok(out)
}

// ── parsing ──

/// Fails on a key outside `allowed`, so a typo cannot silently skip a check.
fn only_keys(object: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<()> {
	for key in object.keys() {
		ensure!(allowed.contains(&key.as_str()), "{what} has an unknown key {key:?}");
	}
	Ok(())
}

fn parse_case(value: &Value) -> Result<Case> {
	let object = value.as_object().context("a case is not an object")?;
	let name = object
		.get("name")
		.and_then(Value::as_str)
		.context("a case has no `name`")?
		.to_string();
	only_keys(
		object,
		&[
			"name",
			"style",
			"source",
			"tileset_maxzoom",
			"tileset_layers",
			"zoom",
			"tile",
			"drawn",
			"kept",
			"widening",
			"error",
			"fields",
		],
		&format!("case {name:?}"),
	)?;
	let parse = || -> Result<Case> {
		let style = parse_style(object.get("style").context("no `style`")?)?;
		let source = match object.get("source") {
			None => Vec::new(),
			Some(value) => string_list(value).context("`source`")?,
		};
		let tileset_maxzoom = match object.get("tileset_maxzoom") {
			None => 14,
			Some(value) => zoom_of(value).context("`tileset_maxzoom`")?,
		};
		let tileset_layers = match object.get("tileset_layers") {
			None => None,
			Some(value) => Some(string_list(value).context("`tileset_layers`")?.into_iter().collect()),
		};
		let fields = match object.get("fields") {
			None => None,
			Some(value) => Some(
				value
					.as_object()
					.context("`fields` is not an object")?
					.iter()
					.map(|(layer, names)| Ok((layer.clone(), string_list(names)?.into_iter().collect())))
					.collect::<Result<_>>()
					.context("`fields`")?,
			),
		};

		let expect = if let Some(error) = object.get("error") {
			for key in ["zoom", "tile", "drawn", "kept", "widening", "fields"] {
				ensure!(!object.contains_key(key), "a case with `error` has no `{key}`");
			}
			Expect::Error(error.as_str().context("`error` is not a string")?.to_string())
		} else {
			parse_tiles(object)?
		};

		Ok(Case {
			name: name.clone(),
			style,
			source,
			tileset_maxzoom,
			tileset_layers,
			fields,
			expect,
		})
	};
	parse().with_context(|| format!("case {name:?}"))
}

/// The style as JSON text: an array of layers is wrapped as `{"layers": […]}`, and a layer without
/// an `id` gets one.
fn parse_style(value: &Value) -> Result<String> {
	let mut style = match value {
		Value::Array(layers) => {
			let mut style = Map::new();
			style.insert("layers".to_string(), Value::Array(layers.clone()));
			style
		}
		Value::Object(style) => style.clone(),
		_ => bail!("`style` is neither an array of layers nor a style object"),
	};
	if let Some(Value::Array(layers)) = style.get_mut("layers") {
		for (index, layer) in layers.iter_mut().enumerate() {
			if let Value::Object(layer) = layer {
				layer
					.entry("id")
					.or_insert_with(|| Value::String(format!("layer-{index}")));
			}
		}
	}
	Ok(Value::Object(style).to_string())
}

fn parse_tiles(object: &Map<String, Value>) -> Result<Expect> {
	let zoom = object.get("zoom").context("no `zoom` (or `error`)")?;
	let (zooms, keyed) = match zoom {
		Value::Array(list) => (list.iter().map(zoom_of).collect::<Result<Vec<_>>>()?, true),
		value => (vec![zoom_of(value)?], false),
	};
	ensure!(!zooms.is_empty(), "`zoom` is an empty list");

	let tile = parse_tile(object.get("tile").context("no `tile`")?).context("`tile`")?;
	let drawn = per_zoom(object.get("drawn").context("no `drawn`")?, &zooms, keyed).context("`drawn`")?;
	let kept = per_zoom(object.get("kept").context("no `kept`")?, &zooms, keyed).context("`kept`")?;

	let widening = match object.get("widening") {
		None => None,
		Some(value) => {
			let widening = value.as_object().context("`widening` is not an object")?;
			only_keys(widening, &["kind", "why"], "`widening`")?;
			let kind = widening
				.get("kind")
				.and_then(Value::as_str)
				.context("`widening` has no `kind`")?;
			ensure!(
				WIDENING_KINDS.contains(&kind),
				"`widening.kind` {kind:?} is none of {WIDENING_KINDS:?}"
			);
			ensure!(
				widening
					.get("why")
					.and_then(Value::as_str)
					.is_some_and(|why| !why.is_empty()),
				"`widening` has no `why`"
			);
			Some(kind.to_string())
		}
	};

	let zooms = zooms
		.into_iter()
		.map(|zoom| (zoom, (drawn[&zoom].clone(), kept[&zoom].clone())))
		.collect();
	Ok(Expect::Tiles { tile, zooms, widening })
}

/// `drawn` or `kept`: one content, or with a list of zooms, one per zoom keyed by it.
fn per_zoom(value: &Value, zooms: &[u8], keyed: bool) -> Result<BTreeMap<u8, Content>> {
	if !keyed {
		return Ok([(zooms[0], parse_content(value)?)].into_iter().collect());
	}
	let object = value
		.as_object()
		.context("with a list of zooms, this is keyed by zoom")?;
	let mut out = BTreeMap::new();
	for zoom in zooms {
		let content = object
			.get(&zoom.to_string())
			.with_context(|| format!("has no entry for zoom {zoom}"))?;
		out.insert(*zoom, parse_content(content).with_context(|| format!("zoom {zoom}"))?);
	}
	ensure!(object.len() == zooms.len(), "has entries for zooms not in `zoom`");
	Ok(out)
}

/// `{"layer": {"1": ["kind"]}}` as [`Content`].
fn parse_content(value: &Value) -> Result<Content> {
	value
		.as_object()
		.context("not an object of layers")?
		.iter()
		.map(|(layer, features)| {
			let features = features
				.as_object()
				.with_context(|| format!("layer {layer:?} is not an object of feature IDs"))?
				.iter()
				.map(|(id, properties)| {
					let id: u64 = id
						.parse()
						.with_context(|| format!("feature ID {id:?} is not a number"))?;
					Ok((id, string_list(properties)?.into_iter().collect()))
				})
				.collect::<Result<_>>()?;
			Ok((layer.clone(), features))
		})
		.collect()
}

/// The input tile: layer → features, each with an `id`, an optional `$geometry`, and properties.
fn parse_tile(value: &Value) -> Result<VectorTile> {
	let mut layers = Vec::new();
	for (name, features) in value.as_object().context("not an object of layers")? {
		let mut ids = BTreeSet::new();
		let mut parsed = Vec::new();
		for feature in features
			.as_array()
			.with_context(|| format!("layer {name:?} is not an array"))?
		{
			let feature = feature.as_object().context("a feature is not an object")?;
			let id = feature
				.get("id")
				.and_then(Value::as_u64)
				.with_context(|| format!("a feature of {name:?} has no numeric `id`"))?;
			ensure!(ids.insert(id), "feature ID {id} is used twice in {name:?}");

			let geometry = match feature.get("$geometry").map(Value::as_str) {
				None | Some(Some("Point")) => Geometry::Point(Point::new(100.0, 100.0)),
				Some(Some("LineString")) => Geometry::LineString(LineString::from(vec![(0.0, 0.0), (100.0, 100.0)])),
				Some(Some("Polygon")) => Geometry::Polygon(Polygon::new(
					LineString::from(vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 0.0)]),
					Vec::new(),
				)),
				Some(other) => bail!("`$geometry` {other:?} is none of Point, LineString, Polygon"),
			};

			let mut properties = GeoProperties::new();
			for (key, value) in feature.iter().filter(|(key, _)| *key != "id" && *key != "$geometry") {
				properties.insert(
					key.clone(),
					geo_value(value).with_context(|| format!("property {key:?}"))?,
				);
			}

			let mut geo_feature = GeoFeature::new(geometry);
			geo_feature.set_id(GeoValue::UInt(id));
			geo_feature.set_properties(properties);
			parsed.push(geo_feature);
		}
		layers.push(VectorTileLayer::from_features(name.clone(), parsed, 4096, 2)?);
	}
	Ok(VectorTile::new(layers))
}

/// A property value: a string, boolean or number, or a typed number such as `{"uint": 2}`.
fn geo_value(value: &Value) -> Result<GeoValue> {
	Ok(match value {
		Value::String(s) => GeoValue::String(s.clone()),
		Value::Bool(b) => GeoValue::Bool(*b),
		Value::Number(n) => match n.as_i64() {
			Some(i) => GeoValue::Int(i),
			None => GeoValue::Double(n.as_f64().context("not a finite number")?),
		},
		Value::Object(typed) => {
			let [(kind, number)] = typed.iter().collect::<Vec<_>>()[..] else {
				bail!("a typed number has exactly one key");
			};
			let number = number.as_f64().context("a typed number holds a number")?;
			#[expect(
				clippy::cast_possible_truncation,
				clippy::cast_sign_loss,
				reason = "test data, written to fit the type it names"
			)]
			match kind.as_str() {
				"int" => GeoValue::Int(number as i64),
				"uint" => GeoValue::UInt(number as u64),
				"float" => GeoValue::Float(number as f32),
				"double" => GeoValue::Double(number),
				other => bail!("typed number {other:?} is none of int, uint, float, double"),
			}
		}
		_ => bail!("{value} is no property value"),
	})
}

fn zoom_of(value: &Value) -> Result<u8> {
	let zoom = value.as_u64().context("a zoom is a whole number")?;
	ensure!(zoom <= 30, "zoom {zoom} is above 30");
	Ok(u8::try_from(zoom)?)
}

fn string_list(value: &Value) -> Result<Vec<String>> {
	value
		.as_array()
		.context("not an array")?
		.iter()
		.map(|item| item.as_str().map(str::to_string).context("not a string"))
		.collect()
}
