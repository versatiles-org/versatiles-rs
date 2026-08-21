//! Machine-readable Shortbread schema model and the embedded reference data.
//!
//! The two `shortbread_*.yaml` files are hand-maintained: `required` and
//! `enum_severity` are validator policy, not facts the specification states, so
//! nothing can generate them. Everything the spec *does* state — the layer set,
//! each layer's geometry and minimum zoom, its property names and types, and the
//! `kind` vocabulary — is checked against the published spec by `check_schema.py`
//! next to them. Run it when a new Shortbread version lands.
//!
//! The YAML is embedded at compile time, so the tool itself stays offline and
//! deterministic.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use versatiles_geometry::{geo::GeoValue, vector_tile::GeomType};

const SCHEMA_1_0: &str = include_str!("shortbread_1_0.yaml");
const SCHEMA_1_1: &str = include_str!("shortbread_1_1.yaml");

/// A concrete Shortbread spec version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaVersion {
	V1_0,
	V1_1,
}

/// A `--schema` selection: which schema family, and optionally which version.
///
/// Parsed from strings like `auto`, `shortbread`, or `shortbread@1.1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaSelector {
	/// Guess both the schema family and its version from the container.
	Auto,
	/// Validate against Shortbread. `version: None` auto-detects the version.
	Shortbread { version: Option<SchemaVersion> },
}

/// Parses a `--schema` value: `auto`, `<family>`, or `<family>@<version>`.
///
/// Returns a `String` error (the form clap expects from a `value_parser`).
/// Today the only family is `shortbread`; `auto` is therefore equivalent to
/// `shortbread` with an auto-detected version, but is kept distinct so future
/// schema families can be guessed here too.
pub fn parse_schema_selector(s: &str) -> std::result::Result<SchemaSelector, String> {
	let s = s.trim();
	if s.eq_ignore_ascii_case("auto") {
		return Ok(SchemaSelector::Auto);
	}
	let (family, version) = match s.split_once('@') {
		Some((family, version)) => (family.trim(), Some(version.trim())),
		None => (s, None),
	};
	match family.to_ascii_lowercase().as_str() {
		"shortbread" => Ok(SchemaSelector::Shortbread {
			version: version.map(parse_shortbread_version).transpose()?,
		}),
		other => Err(format!(
			"unknown schema {other:?}; supported: auto, shortbread, shortbread@1.0, shortbread@1.1"
		)),
	}
}

fn parse_shortbread_version(v: &str) -> std::result::Result<SchemaVersion, String> {
	match v {
		"1.0" => Ok(SchemaVersion::V1_0),
		"1.1" => Ok(SchemaVersion::V1_1),
		other => Err(format!("unknown shortbread version {other:?}; supported: 1.0, 1.1")),
	}
}

/// A parsed Shortbread schema: a set of named layer definitions.
#[derive(Debug, Deserialize)]
pub struct Schema {
	pub version: String,
	pub layers: BTreeMap<String, LayerDef>,
}

/// One layer's expected geometry, minimum zoom, and attribute definitions.
#[derive(Debug, Deserialize)]
pub struct LayerDef {
	pub geometry: GeomKind,
	pub minzoom: u8,
	pub attributes: BTreeMap<String, AttrDef>,
}

/// One attribute's expected value type and (optionally) its allowed values.
#[derive(Debug, Deserialize)]
pub struct AttrDef {
	#[serde(rename = "type")]
	pub ty: AttrType,
	#[serde(default)]
	pub required: bool,
	#[serde(default, rename = "enum")]
	pub enum_values: Option<Vec<String>>,
	#[serde(default)]
	pub enum_severity: EnumSeverity,
}

/// Geometry class a layer is expected to carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum GeomKind {
	Point,
	Line,
	Polygon,
}

impl GeomKind {
	/// Whether an MVT geometry type belongs to this class. `Unknown` (a feature
	/// with no geometry) matches nothing.
	pub fn matches(self, geom_type: GeomType) -> bool {
		matches!(
			(self, geom_type),
			(GeomKind::Point, GeomType::MultiPoint)
				| (GeomKind::Line, GeomType::MultiLineString)
				| (GeomKind::Polygon, GeomType::MultiPolygon)
		)
	}

	pub fn as_str(self) -> &'static str {
		match self {
			GeomKind::Point => "Point",
			GeomKind::Line => "Line",
			GeomKind::Polygon => "Polygon",
		}
	}
}

/// Value type an attribute is expected to hold (the three TileJSON field types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum AttrType {
	String,
	Number,
	Boolean,
}

impl AttrType {
	/// Whether a concrete property value is of this type. `Null` carries no type
	/// information and is accepted against any declared type.
	pub fn matches(self, value: &GeoValue) -> bool {
		match value {
			GeoValue::Null => true,
			GeoValue::String(_) => self == AttrType::String,
			GeoValue::Bool(_) => self == AttrType::Boolean,
			GeoValue::Double(_) | GeoValue::Float(_) | GeoValue::Int(_) | GeoValue::UInt(_) => self == AttrType::Number,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			AttrType::String => "String",
			AttrType::Number => "Number",
			AttrType::Boolean => "Boolean",
		}
	}
}

/// Severity to report when an enum-typed attribute holds a value outside its
/// documented set. `kind`/`admin_level` are `Warn`; the long OSM-derived pois
/// enumerations are `Hint` (they churn faster than the schema).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnumSeverity {
	#[default]
	Warn,
	Hint,
}

impl Schema {
	/// Parses one embedded schema document.
	fn parse(yaml: &str) -> Result<Schema> {
		serde_yaml_ng::from_str(yaml).context("Failed to parse embedded shortbread schema")
	}

	fn v1_0() -> Result<Schema> {
		Schema::parse(SCHEMA_1_0)
	}

	fn v1_1() -> Result<Schema> {
		Schema::parse(SCHEMA_1_1)
	}

	/// Resolves a [`SchemaSelector`] into a concrete schema.
	///
	/// `Auto` and `Shortbread { version: None }` auto-detect the version from
	/// `present`, the container's declared layers and their fields (ties → 1.1).
	/// Only the Shortbread family exists today, so `Auto` resolves to it.
	pub fn resolve(selector: SchemaSelector, present: &[(String, Vec<String>)]) -> Result<Schema> {
		match selector {
			SchemaSelector::Auto | SchemaSelector::Shortbread { version: None } => {
				Schema::resolve_shortbread_auto(present)
			}
			SchemaSelector::Shortbread {
				version: Some(SchemaVersion::V1_0),
			} => Schema::v1_0(),
			SchemaSelector::Shortbread {
				version: Some(SchemaVersion::V1_1),
			} => Schema::v1_1(),
		}
	}

	/// Picks the Shortbread version the container's declared layers and fields fit
	/// best (ties → 1.1, the current spec).
	///
	/// Scoring on layer names alone can never separate the two: 1.0 and 1.1 define
	/// the same 26 layers, so every comparison tied and `auto` silently always
	/// meant 1.1 (issue #241). The versions differ in their *fields* — 1.0 declares
	/// `name_de`/`name_en` on the label layers, 1.1 dropped those and added
	/// `streets.motorcar`/`streets.foot` — so the fields are what decide.
	fn resolve_shortbread_auto(present: &[(String, Vec<String>)]) -> Result<Schema> {
		let s10 = Schema::v1_0()?;
		let s11 = Schema::v1_1()?;
		let score = |s: &Schema| {
			present
				.iter()
				.filter_map(|(layer, fields)| s.layers.get(layer).map(|def| (def, fields)))
				.map(|(def, fields)| 1 + fields.iter().filter(|f| def.attributes.contains_key(*f)).count())
				.sum::<usize>()
		};
		if score(&s10) > score(&s11) { Ok(s10) } else { Ok(s11) }
	}
}

/// Whether a property key is a name variant (`name`, `name_en`, `name:de`, …).
/// These are allowed on any layer and are never reported as unknown attributes.
pub fn is_name_variant(key: &str) -> bool {
	key == "name" || key.starts_with("name_") || key.starts_with("name:")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn both_schemas_parse_and_have_all_layers() {
		for s in [Schema::v1_0().unwrap(), Schema::v1_1().unwrap()] {
			// All 26 documented layers must be present.
			assert_eq!(s.layers.len(), 26, "version {}", s.version);
			let wp = s.layers.get("water_polygons").expect("water_polygons");
			assert_eq!(wp.geometry, GeomKind::Polygon);
			assert_eq!(wp.minzoom, 4);
			let kind = wp.attributes.get("kind").expect("kind attr");
			assert!(kind.required);
			assert!(kind.enum_values.as_ref().unwrap().contains(&"glacier".to_string()));
		}
	}

	#[test]
	fn boundaries_admin_level_enum() {
		let s = Schema::v1_1().unwrap();
		let al = s.layers["boundaries"].attributes.get("admin_level").unwrap();
		assert_eq!(al.ty, AttrType::Number);
		assert_eq!(
			al.enum_values.as_deref(),
			Some(["2".to_string(), "4".to_string()].as_slice())
		);
		assert_eq!(al.enum_severity, EnumSeverity::Warn);
	}

	#[test]
	fn pois_long_enums_are_hints() {
		let s = Schema::v1_1().unwrap();
		let shop = s.layers["pois"].attributes.get("shop").unwrap();
		assert_eq!(shop.enum_severity, EnumSeverity::Hint);
		assert!(!shop.required);
	}

	#[test]
	fn auto_resolves_to_1_1_when_ambiguous() {
		let s = Schema::resolve(SchemaSelector::Auto, &[]).unwrap();
		assert_eq!(s.version, "1.1");
		// `shortbread` (family only) auto-detects the version the same way.
		let s = Schema::resolve(SchemaSelector::Shortbread { version: None }, &[]).unwrap();
		assert_eq!(s.version, "1.1");
	}

	fn layer(name: &str, fields: &[&str]) -> (String, Vec<String>) {
		(name.to_string(), fields.iter().map(|s| (*s).to_string()).collect())
	}

	#[test]
	fn auto_tells_the_versions_apart_by_their_fields() {
		// Both versions define all 26 layers, so the layer names alone always tie
		// and `auto` used to mean 1.1 no matter what it was given (issue #241).
		let layers_only = [layer("streets", &[]), layer("place_labels", &[])];
		assert_eq!(
			Schema::resolve(SchemaSelector::Auto, &layers_only).unwrap().version,
			"1.1"
		);

		// `name_de`/`name_en` exist in 1.0 only.
		let v1_0 = [layer("place_labels", &["kind", "name", "name_de", "name_en"])];
		assert_eq!(Schema::resolve(SchemaSelector::Auto, &v1_0).unwrap().version, "1.0");

		// `motorcar`/`foot` were added in 1.1.
		let v1_1 = [layer("streets", &["kind", "motorcar", "foot"])];
		assert_eq!(Schema::resolve(SchemaSelector::Auto, &v1_1).unwrap().version, "1.1");
	}

	#[test]
	fn streets_carries_the_full_access_group_in_1_1() {
		// The 1.1 spec normalises all four access tags to yes/limited/no; 1.0 has
		// only `bicycle` and `horse`, carrying the raw OSM value.
		let s11 = Schema::v1_1().unwrap();
		for name in ["motorcar", "bicycle", "foot", "horse"] {
			let attr = s11.layers["streets"]
				.attributes
				.get(name)
				.unwrap_or_else(|| panic!("1.1 streets.{name}"));
			assert_eq!(attr.ty, AttrType::String);
			assert_eq!(
				attr.enum_values.as_deref(),
				Some(["yes".to_string(), "limited".to_string(), "no".to_string()].as_slice()),
				"streets.{name}"
			);
		}

		let s10 = Schema::v1_0().unwrap();
		assert!(!s10.layers["streets"].attributes.contains_key("motorcar"));
		assert!(!s10.layers["streets"].attributes.contains_key("foot"));
		assert!(s10.layers["streets"].attributes["bicycle"].enum_values.is_none());
	}

	#[test]
	fn label_layers_inherit_their_kind_vocabulary() {
		// The spec defines these by reference ("see the water_polygons layer for a
		// list of values"), so they had no enum at all and went unvalidated.
		for s in [Schema::v1_0().unwrap(), Schema::v1_1().unwrap()] {
			for (borrower, lender) in [
				("water_polygons_labels", "water_polygons"),
				("water_lines_labels", "water_lines"),
			] {
				let borrowed = s.layers[borrower].attributes["kind"].enum_values.as_deref();
				let lent = s.layers[lender].attributes["kind"].enum_values.as_deref();
				assert!(lent.is_some_and(|v| !v.is_empty()), "{lender} in {}", s.version);
				assert_eq!(borrowed, lent, "{borrower} in {}", s.version);
			}
		}
	}

	#[test]
	fn resolve_honours_explicit_version() {
		let s = Schema::resolve(
			SchemaSelector::Shortbread {
				version: Some(SchemaVersion::V1_0),
			},
			&[],
		)
		.unwrap();
		assert_eq!(s.version, "1.0");
	}

	#[test]
	fn parse_schema_selector_forms() {
		assert_eq!(parse_schema_selector("auto").unwrap(), SchemaSelector::Auto);
		assert_eq!(parse_schema_selector("AUTO").unwrap(), SchemaSelector::Auto);
		assert_eq!(
			parse_schema_selector("shortbread").unwrap(),
			SchemaSelector::Shortbread { version: None }
		);
		assert_eq!(
			parse_schema_selector("shortbread@1.0").unwrap(),
			SchemaSelector::Shortbread {
				version: Some(SchemaVersion::V1_0)
			}
		);
		assert_eq!(
			parse_schema_selector("shortbread@1.1").unwrap(),
			SchemaSelector::Shortbread {
				version: Some(SchemaVersion::V1_1)
			}
		);
		// Unknown family and unknown version are rejected with helpful messages.
		assert!(
			parse_schema_selector("openmaptiles")
				.unwrap_err()
				.contains("unknown schema")
		);
		assert!(
			parse_schema_selector("shortbread@9.9")
				.unwrap_err()
				.contains("unknown shortbread version")
		);
	}

	#[test]
	fn geom_and_type_matchers() {
		assert!(GeomKind::Polygon.matches(GeomType::MultiPolygon));
		assert!(!GeomKind::Polygon.matches(GeomType::MultiLineString));
		assert!(!GeomKind::Point.matches(GeomType::Unknown));
		assert!(AttrType::Number.matches(&GeoValue::UInt(5)));
		assert!(AttrType::String.matches(&GeoValue::Null));
		assert!(!AttrType::Boolean.matches(&GeoValue::from("x")));
	}

	#[test]
	fn name_variants() {
		assert!(is_name_variant("name"));
		assert!(is_name_variant("name_en"));
		assert!(is_name_variant("name:de"));
		assert!(!is_name_variant("kind"));
	}
}
