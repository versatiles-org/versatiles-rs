//! Per-tile Shortbread conformance checks.
//!
//! [`analyze_tile`] is pure and CPU-bound; it runs on the stream worker pool and
//! returns a compact [`Issue`] list that the caller folds into a `Registry`.

use super::report::{Issue, Rule, Severity};
use super::schema::{EnumSeverity, LayerDef, Schema, is_name_variant};
use versatiles_core::TileCoord;
use versatiles_geometry::{
	geo::GeoValue,
	vector_tile::{GeomType, VectorTile, VectorTileLayer},
};

/// Validates one decoded vector tile against `schema`, returning every finding.
pub fn analyze_tile(coord: TileCoord, vt: &VectorTile, schema: &Schema) -> Vec<Issue> {
	let mut issues = Vec::new();
	for layer in &vt.layers {
		match schema.layers.get(&layer.name) {
			None => issues.push(Issue::new(Severity::Warn, Rule::UnknownLayer, &layer.name, coord)),
			Some(def) => check_layer(coord, layer, def, &mut issues),
		}
	}
	issues
}

fn check_layer(coord: TileCoord, layer: &VectorTileLayer, def: &LayerDef, issues: &mut Vec<Issue>) {
	let name = layer.name.as_str();

	if layer.extent != 4096 {
		issues.push(Issue::new(Severity::Hint, Rule::BadExtent, name, coord).detail(format!("extent {}", layer.extent)));
	}
	if coord.level < def.minzoom {
		issues.push(
			Issue::new(Severity::Warn, Rule::BelowMinzoom, name, coord)
				.detail(format!("zoom {} < minzoom {}", coord.level, def.minzoom)),
		);
	}

	let keys = &layer.property_manager.key.list;
	let vals = &layer.property_manager.val.list;

	for feature in &layer.features {
		// Geometry: a feature carrying actual geometry must match the layer's
		// class. `Unknown` is the spec's "no geometry" form — left to the MVT
		// structural validator, not a schema concern.
		if feature.geom_type != GeomType::Unknown && !def.geometry.matches(feature.geom_type) {
			issues.push(
				Issue::new(Severity::Error, Rule::WrongGeometry, name, coord).detail(format!(
					"expected {}, got {:?}",
					def.geometry.as_str(),
					feature.geom_type
				)),
			);
		}

		// Required attributes must be present on every feature.
		for (attr_name, attr) in &def.attributes {
			if attr.required && !feature_has_key(&feature.tag_ids, keys, attr_name) {
				issues.push(Issue::new(Severity::Error, Rule::MissingRequired, name, coord).attr(attr_name));
			}
		}

		// Per key/value pair: unknown attribute, wrong type, bad enum value.
		for pair in feature.tag_ids.chunks_exact(2) {
			let (Some(key), Some(value)) = (keys.get(pair[0] as usize), vals.get(pair[1] as usize)) else {
				continue; // malformed index — structural decode already vetted this
			};
			match def.attributes.get(key) {
				None => {
					if !is_name_variant(key) {
						issues.push(Issue::new(Severity::Warn, Rule::UnknownAttribute, name, coord).attr(key));
					}
				}
				Some(attr) => {
					if !attr.ty.matches(value) {
						issues.push(
							Issue::new(Severity::Error, Rule::WrongType, name, coord)
								.attr(key)
								.detail(format!("expected {}, got {}", attr.ty.as_str(), value_type_name(value))),
						);
					} else if let Some(allowed) = &attr.enum_values
						&& let Some(plain) = geo_value_plain(value)
						&& !allowed.iter().any(|a| a == &plain)
					{
						let severity = match attr.enum_severity {
							EnumSeverity::Warn => Severity::Warn,
							EnumSeverity::Hint => Severity::Hint,
						};
						issues.push(
							Issue::new(severity, Rule::BadEnumValue, name, coord)
								.attr(key)
								.value(&plain),
						);
					}
				}
			}
		}
	}
}

/// Whether a feature's tag list contains `key`.
fn feature_has_key(tag_ids: &[u32], keys: &[String], key: &str) -> bool {
	tag_ids
		.chunks_exact(2)
		.any(|pair| keys.get(pair[0] as usize).is_some_and(|k| k == key))
}

/// The scalar string a value compares as for enum membership. `Null` and the
/// empty string both mean "attribute absent" (some encoders store `""` rather
/// than dropping the key) and are skipped.
fn geo_value_plain(v: &GeoValue) -> Option<String> {
	match v {
		GeoValue::Null => None,
		GeoValue::String(s) if s.is_empty() => None,
		GeoValue::String(s) => Some(s.clone()),
		GeoValue::Bool(b) => Some(b.to_string()),
		GeoValue::Int(i) => Some(i.to_string()),
		GeoValue::UInt(u) => Some(u.to_string()),
		GeoValue::Float(f) => Some(f.to_string()),
		GeoValue::Double(d) => Some(d.to_string()),
	}
}

/// Human name of a value's type, for `wrong type` messages.
fn value_type_name(v: &GeoValue) -> &'static str {
	match v {
		GeoValue::String(_) => "String",
		GeoValue::Bool(_) => "Boolean",
		GeoValue::Null => "Null",
		GeoValue::Int(_) | GeoValue::UInt(_) | GeoValue::Float(_) | GeoValue::Double(_) => "Number",
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{Coord, Geometry, LineString, Point, Polygon};
	use versatiles_geometry::geo::{GeoFeature, GeoProperties};

	fn schema() -> Schema {
		Schema::resolve(super::super::schema::SchemaVersion::V1_1, &[]).unwrap()
	}

	fn coord() -> TileCoord {
		TileCoord::new(14, 100, 100).unwrap()
	}

	fn square() -> Geometry<f64> {
		Geometry::Polygon(Polygon::new(
			LineString::new(vec![
				Coord { x: 0.0, y: 0.0 },
				Coord { x: 4.0, y: 0.0 },
				Coord { x: 4.0, y: 4.0 },
				Coord { x: 0.0, y: 4.0 },
				Coord { x: 0.0, y: 0.0 },
			]),
			vec![],
		))
	}

	fn feature(geometry: Geometry<f64>, props: Vec<(&str, GeoValue)>) -> GeoFeature {
		GeoFeature {
			id: None,
			geometry,
			properties: GeoProperties::from(props),
		}
	}

	/// Builds a one-layer tile from high-level features.
	fn tile(layer_name: &str, features: Vec<GeoFeature>) -> VectorTile {
		let layer = VectorTileLayer::from_features(layer_name.to_string(), features, 4096, 1).unwrap();
		VectorTile { layers: vec![layer] }
	}

	fn rules(issues: &[Issue]) -> Vec<Rule> {
		issues.iter().map(|i| i.rule).collect()
	}

	#[test]
	fn clean_feature_has_no_issues() {
		let vt = tile(
			"water_polygons",
			vec![feature(square(), vec![("kind", GeoValue::from("water"))])],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(issues.is_empty(), "expected clean, got {issues:?}");
	}

	#[test]
	fn missing_required_kind_is_error() {
		let vt = tile(
			"water_polygons",
			vec![feature(square(), vec![("way_area", GeoValue::from(5.0))])],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(rules(&issues).contains(&Rule::MissingRequired));
		assert!(issues.iter().any(|i| i.severity == Severity::Error));
	}

	#[test]
	fn unknown_layer_is_warning() {
		let vt = tile("totally_made_up", vec![feature(square(), vec![])]);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert_eq!(rules(&issues), vec![Rule::UnknownLayer]);
		assert_eq!(issues[0].severity, Severity::Warn);
	}

	#[test]
	fn wrong_geometry_is_error() {
		// water_polygons expects Polygon; give it a Point.
		let vt = tile(
			"water_polygons",
			vec![feature(
				Geometry::Point(Point::new(1.0, 1.0)),
				vec![("kind", GeoValue::from("water"))],
			)],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(rules(&issues).contains(&Rule::WrongGeometry));
	}

	#[test]
	fn unknown_attribute_is_warning() {
		let vt = tile(
			"water_polygons",
			vec![feature(
				square(),
				vec![("kind", GeoValue::from("water")), ("bogus", GeoValue::from("x"))],
			)],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(
			issues
				.iter()
				.any(|i| i.rule == Rule::UnknownAttribute && i.attr.as_deref() == Some("bogus"))
		);
	}

	#[test]
	fn name_variants_are_allowed() {
		let vt = tile(
			"place_labels",
			vec![feature(
				Geometry::Point(Point::new(1.0, 1.0)),
				vec![
					("kind", GeoValue::from("city")),
					("name", GeoValue::from("Berlin")),
					("name_en", GeoValue::from("Berlin")),
					("name:de", GeoValue::from("Berlin")),
				],
			)],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(
			!issues.iter().any(|i| i.rule == Rule::UnknownAttribute),
			"name variants must not be flagged: {issues:?}"
		);
	}

	#[test]
	fn wrong_type_for_admin_level_is_error() {
		// boundaries.admin_level is a Number; a string value is a type error.
		let vt = tile(
			"boundaries",
			vec![feature(
				Geometry::LineString(LineString::new(vec![
					Coord { x: 0.0, y: 0.0 },
					Coord { x: 1.0, y: 1.0 },
				])),
				vec![("admin_level", GeoValue::from("two"))],
			)],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(
			issues
				.iter()
				.any(|i| i.rule == Rule::WrongType && i.attr.as_deref() == Some("admin_level"))
		);
	}

	#[test]
	fn bad_kind_value_is_warning() {
		let vt = tile(
			"land",
			vec![feature(square(), vec![("kind", GeoValue::from("not_a_real_kind"))])],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		let e = issues
			.iter()
			.find(|i| i.rule == Rule::BadEnumValue)
			.expect("bad enum value");
		assert_eq!(e.severity, Severity::Warn);
		assert_eq!(e.value.as_deref(), Some("not_a_real_kind"));
	}

	#[test]
	fn valid_admin_level_integer_passes_enum() {
		let vt = tile(
			"boundaries",
			vec![feature(
				Geometry::LineString(LineString::new(vec![
					Coord { x: 0.0, y: 0.0 },
					Coord { x: 1.0, y: 1.0 },
				])),
				vec![("admin_level", GeoValue::from(2u64))],
			)],
		);
		let issues = analyze_tile(coord(), &vt, &schema());
		assert!(
			!issues.iter().any(|i| i.rule == Rule::BadEnumValue),
			"admin_level=2 is valid: {issues:?}"
		);
	}
}
