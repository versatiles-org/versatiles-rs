#![allow(dead_code)]
//! Defines `GeoFeature`, a single GeoJSON-like feature with optional `id`, a `Geometry`,
//! and a set of typed properties. Geometry is a `geo_types::Geometry<f64>`; helpers in
//! `crate::ext` handle GeoJSON output, projection, and validation.

use super::{GeoProperties, GeoValue};
use crate::ext::geometry_to_json;
use geo_types::{Geometry, MultiLineString, MultiPoint, MultiPolygon, Point};
use std::fmt::Debug;
use versatiles_core::json::{JsonObject, JsonValue};

/// A single geographic feature consisting of an optional `id`, a `Geometry`, and `GeoProperties`.
///
/// This mirrors a GeoJSON *Feature*: `geometry` holds the spatial data, `properties` stores
/// arbitrary typed attributes, and `id` is optional.
#[derive(Clone, Debug)]
pub struct GeoFeature {
	/// Optional feature identifier. If present, it is emitted as the GeoJSON `id` field.
	pub id: Option<GeoValue>,
	/// The feature's spatial component.
	pub geometry: Geometry<f64>,
	/// Key–value attributes associated with the feature (emitted as GeoJSON `properties`).
	pub properties: GeoProperties,
}

impl GeoFeature {
	/// Creates a new `GeoFeature` with the given `geometry`, no `id`, and empty `properties`.
	#[must_use]
	pub fn new(geometry: Geometry<f64>) -> Self {
		Self {
			id: None,
			geometry,
			properties: GeoProperties::new(),
		}
	}

	/// Sets the optional identifier of the feature (serialized as GeoJSON `id`).
	pub fn set_id(&mut self, id: GeoValue) {
		self.id = Some(id);
	}

	/// Replaces the entire properties map with the provided `GeoProperties`.
	pub fn set_properties(&mut self, properties: GeoProperties) {
		self.properties = properties;
	}

	/// Inserts or updates a single property value.
	pub fn set_property<T>(&mut self, key: String, value: T)
	where
		GeoValue: From<T>,
	{
		self.properties.insert(key, GeoValue::from(value));
	}

	/// Converts the inner geometry to its *single* variant when it is a multi-geometry
	/// containing exactly one element. No-op otherwise.
	pub fn to_single_geometry(&mut self) {
		take_and_replace(&mut self.geometry, into_single_geometry);
	}

	/// Converts the inner geometry to its *multi* variant when it is a single-geometry.
	/// Multi variants are unchanged.
	pub fn to_multi_geometry(&mut self) {
		take_and_replace(&mut self.geometry, into_multi_geometry);
	}

	/// Serializes the feature into a GeoJSON-compatible `JsonObject`.
	#[must_use]
	pub fn to_json(&self, precision: Option<u8>) -> JsonObject {
		let mut json = JsonObject::new();
		json.set("type", JsonValue::from("Feature"));
		if let Some(id) = &self.id {
			json.set("id", id.to_json());
		}
		json.set("geometry", JsonValue::from(geometry_to_json(&self.geometry, precision)));
		json.set("properties", self.properties.to_json());
		json
	}

	#[cfg(any(test, feature = "test"))]
	#[must_use]
	/// Test helper that returns a deterministic example feature.
	pub fn new_example() -> Self {
		Self {
			id: Some(GeoValue::from(13)),
			geometry: example_geometry(),
			properties: GeoProperties::from(vec![
				("name", GeoValue::from("Nice")),
				("population", GeoValue::from(348085)),
				("is_nice", GeoValue::from(true)),
			]),
		}
	}
}

/// Lifts a single geometry into its multi equivalent. Multi inputs are returned unchanged.
/// Other variants (`Line`, `Rect`, `Triangle`, `GeometryCollection`) pass through.
#[must_use]
pub fn into_multi_geometry(g: Geometry<f64>) -> Geometry<f64> {
	match g {
		Geometry::Point(p) => Geometry::MultiPoint(MultiPoint(vec![p])),
		Geometry::LineString(ls) => Geometry::MultiLineString(MultiLineString(vec![ls])),
		Geometry::Polygon(p) => Geometry::MultiPolygon(MultiPolygon(vec![p])),
		other => other,
	}
}

/// Unwraps a multi-geometry to its single equivalent when it has exactly one element.
/// Single inputs and other variants are returned unchanged.
#[must_use]
pub fn into_single_geometry(g: Geometry<f64>) -> Geometry<f64> {
	match g {
		Geometry::MultiPoint(MultiPoint(mut v)) if v.len() == 1 => Geometry::Point(v.pop().expect("len == 1")),
		Geometry::MultiLineString(MultiLineString(mut v)) if v.len() == 1 => {
			Geometry::LineString(v.pop().expect("len == 1"))
		}
		Geometry::MultiPolygon(MultiPolygon(mut v)) if v.len() == 1 => Geometry::Polygon(v.pop().expect("len == 1")),
		other => other,
	}
}

fn take_and_replace<F>(slot: &mut Geometry<f64>, f: F)
where
	F: FnOnce(Geometry<f64>) -> Geometry<f64>,
{
	let dummy: Geometry<f64> = Geometry::Point(Point::new(0.0, 0.0));
	let owned = std::mem::replace(slot, dummy);
	*slot = f(owned);
}

/// Test helper: returns a deterministic example `MultiPolygon` geometry with holes.
#[cfg(any(test, feature = "test"))]
#[must_use]
pub fn example_geometry() -> Geometry<f64> {
	use geo_types::{LineString, Polygon};

	fn polygon_from_rings(rings: &[Vec<[f64; 2]>]) -> Polygon<f64> {
		let mut iter = rings.iter().map(|ring| LineString::from(ring.clone()));
		let exterior = iter.next().expect("polygon has at least an exterior");
		let interiors = iter.collect();
		Polygon::new(exterior, interiors)
	}

	let polygon_a = polygon_from_rings(&[
		vec![[0.0, 0.0], [5.0, 0.0], [2.5, 4.0], [0.0, 0.0]],
		vec![[2.0, 1.0], [2.5, 2.0], [3.0, 1.0], [2.0, 1.0]],
	]);
	let polygon_b = polygon_from_rings(&[
		vec![[6.0, 0.0], [9.0, 0.0], [9.0, 4.0], [6.0, 4.0], [6.0, 0.0]],
		vec![[7.0, 1.0], [7.0, 3.0], [8.0, 3.0], [8.0, 1.0], [7.0, 1.0]],
	]);
	Geometry::MultiPolygon(MultiPolygon(vec![polygon_a, polygon_b]))
}

/// Wrap a `geo_types::Geometry<f64>` directly into a `GeoFeature`.
impl From<Geometry<f64>> for GeoFeature {
	fn from(geometry: Geometry<f64>) -> Self {
		Self::new(geometry)
	}
}

/// Wrap a `geo_types::MultiPolygon<f64>` (commonly produced by tile_outline) into a
/// `GeoFeature` with no id and empty properties.
impl From<MultiPolygon<f64>> for GeoFeature {
	fn from(geometry: MultiPolygon<f64>) -> Self {
		Self::new(Geometry::MultiPolygon(geometry))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{Coord, MultiPolygon, polygon};

	#[test]
	fn new_sets_defaults() {
		let geom = example_geometry();
		let f = GeoFeature::new(geom.clone());
		assert!(f.id.is_none());
		assert_eq!(f.geometry, geom);
		assert!(f.properties.is_empty());
	}

	#[test]
	fn set_id_and_properties_work() {
		let mut f = GeoFeature::new(example_geometry());
		f.set_id(GeoValue::from(42));
		assert_eq!(f.id, Some(GeoValue::from(42)));

		let mut props = GeoProperties::new();
		props.insert("name".into(), GeoValue::from("Nice"));
		props.insert("population".into(), GeoValue::from(348_085));
		f.set_properties(props.clone());
		assert_eq!(f.properties, props);
	}

	#[test]
	fn set_property_inserts_values_of_various_types() {
		let mut f = GeoFeature::new(example_geometry());
		f.set_property("a".into(), 1u32);
		f.set_property("b".into(), "text");
		f.set_property("c".into(), true);
		assert_eq!(f.properties.get("a"), Some(&GeoValue::from(1u32)));
		assert_eq!(f.properties.get("b"), Some(&GeoValue::from("text")));
		assert_eq!(f.properties.get("c"), Some(&GeoValue::from(true)));
	}

	#[test]
	fn to_json_contains_type_geometry_properties_and_optional_id() {
		let mut f = GeoFeature::new_example();
		let j = f.to_json(None);
		assert_eq!(j.string("type").unwrap(), Some("Feature".into()));
		assert!(j.number("id").unwrap().is_some());
		assert!(j.object("geometry").unwrap().is_some());
		assert!(j.object("properties").unwrap().is_some());

		f.id = None;
		let j2 = f.to_json(None);
		assert_eq!(j2.string("type").unwrap(), Some("Feature".into()));
		assert!(j2.number("id").unwrap().is_none());
	}

	#[test]
	fn from_multipolygon_builds_feature() {
		let poly = polygon![(x: 0.0, y: 0.0), (x: 1.0, y: 0.0), (x: 1.0, y: 1.0), (x: 0.0, y: 1.0), (x: 0.0, y: 0.0)];
		let mp = MultiPolygon(vec![poly]);
		let f: GeoFeature = mp.into();
		assert!(f.id.is_none());
		assert!(f.properties.is_empty());
		let j = f.to_json(None);
		assert_eq!(j.string("type").unwrap(), Some("Feature".into()));
		assert!(j.object("geometry").unwrap().is_some());
	}

	#[test]
	fn into_multi_lifts_singles() {
		let pt: Geometry<f64> = Geometry::Point(Point::new(1.0, 2.0));
		match into_multi_geometry(pt) {
			Geometry::MultiPoint(_) => {}
			other => panic!("expected MultiPoint, got {other:?}"),
		}
	}

	#[test]
	fn into_single_unwraps_singletons() {
		let mp = Geometry::MultiPoint(MultiPoint(vec![Point(Coord { x: 1.0, y: 2.0 })]));
		match into_single_geometry(mp) {
			Geometry::Point(_) => {}
			other => panic!("expected Point, got {other:?}"),
		}
	}

	#[test]
	fn into_single_keeps_multi_with_more_than_one() {
		let mp = Geometry::MultiPoint(MultiPoint(vec![
			Point(Coord { x: 1.0, y: 2.0 }),
			Point(Coord { x: 3.0, y: 4.0 }),
		]));
		match into_single_geometry(mp) {
			Geometry::MultiPoint(v) => assert_eq!(v.0.len(), 2),
			other => panic!("expected MultiPoint, got {other:?}"),
		}
	}
}
