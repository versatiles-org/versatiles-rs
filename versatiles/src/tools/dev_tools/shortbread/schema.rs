//! Machine-readable Shortbread schema model and the embedded reference data.
//!
//! The two `shortbread_*.yaml` files are generated from the official spec
//! markdown by `generate_schema.py` (kept alongside them for provenance) and
//! hand-reviewed before commit. They are embedded at compile time so the tool
//! stays offline and deterministic.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use versatiles_geometry::{geo::GeoValue, vector_tile::GeomType};

const SCHEMA_1_0: &str = include_str!("shortbread_1_0.yaml");
const SCHEMA_1_1: &str = include_str!("shortbread_1_1.yaml");

/// Which Shortbread spec version to validate against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum SchemaVersion {
	#[value(name = "1.0")]
	V1_0,
	#[value(name = "1.1")]
	V1_1,
	/// Pick the version whose layer set best matches the container.
	Auto,
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

	/// Resolves a [`SchemaVersion`] into a concrete schema. For [`SchemaVersion::Auto`],
	/// the version whose layer set best overlaps `present_layers` wins (ties → 1.1).
	pub fn resolve(version: SchemaVersion, present_layers: &[String]) -> Result<Schema> {
		match version {
			SchemaVersion::V1_0 => Schema::v1_0(),
			SchemaVersion::V1_1 => Schema::v1_1(),
			SchemaVersion::Auto => {
				let s10 = Schema::v1_0()?;
				let s11 = Schema::v1_1()?;
				let score = |s: &Schema| present_layers.iter().filter(|n| s.layers.contains_key(*n)).count();
				// Prefer 1.1 on ties: it is a superset and the current spec.
				if score(&s10) > score(&s11) { Ok(s10) } else { Ok(s11) }
			}
		}
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
		let s = Schema::resolve(SchemaVersion::Auto, &[]).unwrap();
		assert_eq!(s.version, "1.1");
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
