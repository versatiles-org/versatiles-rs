//! Cutting a geometry to a bounding box.

use geo::BooleanOps;
use geo_types::{Coord, Geometry, MultiLineString, MultiPoint, MultiPolygon, Point, Rect};
use versatiles_core::GeoBBox;

/// The part of `geometry` inside `bbox`, both in degrees, or `None` where
/// nothing of it is inside.
///
/// Points on the edge count as inside. Lines and polygons are cut at the
/// edge, so a polygon that reaches out of the bbox gets a new edge along it.
#[must_use]
pub fn clip_to_bbox(geometry: &Geometry<f64>, bbox: &GeoBBox) -> Option<Geometry<f64>> {
	let rect = Rect::new(
		Coord {
			x: bbox.x_min,
			y: bbox.y_min,
		},
		Coord {
			x: bbox.x_max,
			y: bbox.y_max,
		},
	);
	let inside =
		|p: &Point<f64>| (bbox.x_min..=bbox.x_max).contains(&p.x()) && (bbox.y_min..=bbox.y_max).contains(&p.y());
	let window = MultiPolygon(vec![rect.to_polygon()]);
	let lines = |lines: MultiLineString<f64>| {
		let clipped = window.clip(&lines, false);
		(!clipped.0.is_empty()).then_some(Geometry::MultiLineString(clipped))
	};
	let polygons = |polygons: MultiPolygon<f64>| {
		let clipped = polygons.intersection(&window);
		(!clipped.0.is_empty()).then_some(Geometry::MultiPolygon(clipped))
	};

	match geometry {
		Geometry::Point(p) => inside(p).then(|| geometry.clone()),
		Geometry::MultiPoint(points) => {
			let kept: Vec<Point<f64>> = points.0.iter().filter(|p| inside(p)).copied().collect();
			(!kept.is_empty()).then_some(Geometry::MultiPoint(MultiPoint(kept)))
		}
		Geometry::LineString(line) => lines(MultiLineString(vec![line.clone()])),
		Geometry::MultiLineString(multi) => lines(multi.clone()),
		Geometry::Polygon(polygon) => polygons(MultiPolygon(vec![polygon.clone()])),
		Geometry::MultiPolygon(multi) => polygons(multi.clone()),
		Geometry::Rect(r) => polygons(MultiPolygon(vec![r.to_polygon()])),
		Geometry::Triangle(t) => polygons(MultiPolygon(vec![t.to_polygon()])),
		Geometry::Line(line) => lines(MultiLineString(vec![(*line).into()])),
		Geometry::GeometryCollection(collection) => {
			let kept: Vec<Geometry<f64>> = collection.iter().filter_map(|g| clip_to_bbox(g, bbox)).collect();
			(!kept.is_empty()).then_some(Geometry::GeometryCollection(geo_types::GeometryCollection(kept)))
		}
	}
}

#[cfg(test)]
mod tests {
	use geo::Area;
	use geo_types::{LineString, Polygon, line_string, polygon};

	use super::*;

	fn bbox() -> GeoBBox {
		GeoBBox::new(0.0, 0.0, 10.0, 10.0).unwrap()
	}

	#[test]
	fn points_outside_are_dropped() {
		let points = Geometry::MultiPoint(MultiPoint(vec![
			Point::new(5.0, 5.0),
			Point::new(10.0, 0.0),
			Point::new(11.0, 5.0),
		]));
		let Some(Geometry::MultiPoint(kept)) = clip_to_bbox(&points, &bbox()) else {
			panic!("points should remain");
		};
		assert_eq!(kept.0.len(), 2, "the point on the edge counts as inside");
		assert!(clip_to_bbox(&Geometry::Point(Point::new(-1.0, 5.0)), &bbox()).is_none());
	}

	#[test]
	fn a_line_is_cut_at_the_edge() {
		let line = Geometry::LineString(line_string![(x: -5.0, y: 5.0), (x: 5.0, y: 5.0)]);
		let Some(Geometry::MultiLineString(cut)) = clip_to_bbox(&line, &bbox()) else {
			panic!("part of the line is inside");
		};
		let coords: Vec<(f64, f64)> = cut.0[0].coords().map(|c| (c.x, c.y)).collect();
		assert_eq!(coords, vec![(0.0, 5.0), (5.0, 5.0)]);
		let outside: LineString<f64> = line_string![(x: 20.0, y: 20.0), (x: 30.0, y: 30.0)];
		assert!(clip_to_bbox(&Geometry::LineString(outside), &bbox()).is_none());
	}

	#[test]
	fn a_polygon_is_cut_at_the_edge() {
		let square: Polygon<f64> = polygon![(x: 5.0, y: 5.0), (x: 15.0, y: 5.0), (x: 15.0, y: 15.0), (x: 5.0, y: 15.0)];
		let Some(Geometry::MultiPolygon(cut)) = clip_to_bbox(&Geometry::Polygon(square), &bbox()) else {
			panic!("a quarter of the square is inside");
		};
		assert!((cut.unsigned_area() - 25.0).abs() < 1e-9);
	}
}
