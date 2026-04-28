//! GeoJSON output helpers for `geo_types` geometries.
//!
//! These functions render `geo_types` values into versatiles' [`JsonValue`]/[`JsonObject`]
//! types matching the GeoJSON `{"type": "...", "coordinates": ...}` shape.

use geo_types::{
	Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use versatiles_core::json::{JsonObject, JsonValue};

/// Render a single coordinate as a GeoJSON `[x, y]` array. If `precision` is given,
/// values are rounded to that many fractional digits.
#[must_use]
pub fn coord_to_json(c: &Coord<f64>, precision: Option<u8>) -> JsonValue {
	if let Some(prec) = precision {
		let factor = 10f64.powi(i32::from(prec));
		let x = (c.x * factor).round() / factor;
		let y = (c.y * factor).round() / factor;
		JsonValue::from([x, y])
	} else {
		JsonValue::from([c.x, c.y])
	}
}

fn point_coords(p: &Point<f64>, precision: Option<u8>) -> JsonValue {
	coord_to_json(&p.0, precision)
}

fn line_string_coords(ls: &LineString<f64>, precision: Option<u8>) -> JsonValue {
	JsonValue::from(ls.0.iter().map(|c| coord_to_json(c, precision)).collect::<Vec<_>>())
}

fn polygon_coords(p: &Polygon<f64>, precision: Option<u8>) -> JsonValue {
	let mut rings = Vec::with_capacity(1 + p.interiors().len());
	rings.push(line_string_coords(p.exterior(), precision));
	for interior in p.interiors() {
		rings.push(line_string_coords(interior, precision));
	}
	JsonValue::from(rings)
}

fn multi_point_coords(mp: &MultiPoint<f64>, precision: Option<u8>) -> JsonValue {
	JsonValue::from(mp.0.iter().map(|p| coord_to_json(&p.0, precision)).collect::<Vec<_>>())
}

fn multi_line_string_coords(ml: &MultiLineString<f64>, precision: Option<u8>) -> JsonValue {
	JsonValue::from(
		ml.0
			.iter()
			.map(|ls| line_string_coords(ls, precision))
			.collect::<Vec<_>>(),
	)
}

fn multi_polygon_coords(mp: &MultiPolygon<f64>, precision: Option<u8>) -> JsonValue {
	JsonValue::from(mp.0.iter().map(|p| polygon_coords(p, precision)).collect::<Vec<_>>())
}

/// Returns the GeoJSON `type` name (e.g. `"Polygon"`, `"MultiPoint"`) for a `Geometry`.
///
/// `Line` is reported as `"LineString"`, `Rect`/`Triangle` as `"Polygon"` — these aren't
/// canonical GeoJSON types but the corresponding output `coordinates` shape matches.
#[must_use]
pub fn type_name(g: &Geometry<f64>) -> &'static str {
	match g {
		Geometry::Point(_) => "Point",
		Geometry::Line(_) | Geometry::LineString(_) => "LineString",
		Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => "Polygon",
		Geometry::MultiPoint(_) => "MultiPoint",
		Geometry::MultiLineString(_) => "MultiLineString",
		Geometry::MultiPolygon(_) => "MultiPolygon",
		Geometry::GeometryCollection(_) => "GeometryCollection",
	}
}

/// Render any `Geometry` variant into a GeoJSON-shaped object.
#[must_use]
pub fn geometry_to_json(g: &Geometry<f64>, precision: Option<u8>) -> JsonObject {
	let mut obj = JsonObject::new();
	let (name, coordinates_or_geometries) = match g {
		Geometry::Point(p) => ("Point", point_coords(p, precision)),
		Geometry::LineString(ls) => ("LineString", line_string_coords(ls, precision)),
		Geometry::Polygon(p) => ("Polygon", polygon_coords(p, precision)),
		Geometry::MultiPoint(mp) => ("MultiPoint", multi_point_coords(mp, precision)),
		Geometry::MultiLineString(ml) => ("MultiLineString", multi_line_string_coords(ml, precision)),
		Geometry::MultiPolygon(mp) => ("MultiPolygon", multi_polygon_coords(mp, precision)),
		Geometry::Line(l) => {
			let ls = LineString::new(vec![l.start, l.end]);
			("LineString", line_string_coords(&ls, precision))
		}
		Geometry::Rect(r) => ("Polygon", polygon_coords(&r.to_polygon(), precision)),
		Geometry::Triangle(t) => ("Polygon", polygon_coords(&t.to_polygon(), precision)),
		Geometry::GeometryCollection(gc) => {
			obj.set("type", JsonValue::from("GeometryCollection"));
			obj.set("geometries", geometry_collection_to_json_array(gc, precision));
			return obj;
		}
	};
	obj.set("type", JsonValue::from(name));
	obj.set("coordinates", coordinates_or_geometries);
	obj
}

fn geometry_collection_to_json_array(gc: &GeometryCollection<f64>, precision: Option<u8>) -> JsonValue {
	JsonValue::from(
		gc.0
			.iter()
			.map(|g| JsonValue::from(geometry_to_json(g, precision)))
			.collect::<Vec<_>>(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn point_serialization() {
		let g: Geometry<f64> = Point::new(1.5, 2.5).into();
		let obj = geometry_to_json(&g, None);
		assert_eq!(obj.get("type").unwrap().as_str().unwrap(), "Point");
		assert!(obj.get("coordinates").is_some());
	}

	#[test]
	fn polygon_serialization() {
		let exterior = LineString::from(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]);
		let g: Geometry<f64> = Polygon::new(exterior, vec![]).into();
		let obj = geometry_to_json(&g, None);
		assert_eq!(obj.get("type").unwrap().as_str().unwrap(), "Polygon");
	}

	#[test]
	fn precision_rounds_coordinates() {
		let c = Coord { x: 1.23456, y: 9.87654 };
		assert_eq!(coord_to_json(&c, Some(2)), JsonValue::from([1.23, 9.88]));
	}

	#[test]
	fn no_precision_keeps_full_value() {
		let c = Coord { x: 1.23456, y: 9.87654 };
		assert_eq!(coord_to_json(&c, None), JsonValue::from([1.23456, 9.87654]));
	}

	#[test]
	fn type_name_dispatches() {
		assert_eq!(type_name(&Geometry::Point(Point::new(0.0, 0.0))), "Point");
		assert_eq!(
			type_name(&Geometry::LineString(LineString::from(vec![[0.0, 0.0], [1.0, 1.0]]))),
			"LineString"
		);
	}
}
