use ab_glyph::{Font, FontArc, Outline, OutlineCurve::*, Point};
use anyhow::Result;
use std::{f64::consts::PI, ops::Div, sync::LazyLock, vec};
use versatiles_core::TileCoord;
use versatiles_derive::context;
use versatiles_geometry::{
	geo::*,
	vector_tile::{VectorTile, VectorTileLayer},
};

static FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("./trim.ttf")).unwrap());

#[context("Creating debug vector tile for coord {:?}", coord)]
pub fn create_debug_vector_tile(coord: &TileCoord) -> Result<VectorTile> {
	Ok(VectorTile::new(vec![
		get_background_layer()?,
		draw_text("debug_z", 140.0, format!("z:{}", coord.level)),
		draw_text("debug_x", 190.0, format!("x:{}", coord.x)),
		draw_text("debug_y", 240.0, format!("y:{}", coord.y)),
	]))
}

fn draw_text(name: &str, y: f32, text: String) -> VectorTileLayer {
	let font: &FontArc = &FONT;

	let mut features: Vec<GeoFeature> = Vec::new();
	let height = font.height_unscaled();
	let scale: f32 = 80.0 / height;

	let mut position = Point { x: 100.0, y };

	let get_char_as_feature = |outline: Outline, position: &Point| {
		let mut mls = MultiLineStringGeometry::new();
		for curve in outline.curves {
			let points = match curve {
				Line(p0, p1) => vec![p0, p1],
				Quad(p0, c0, p1) => draw_quad(p0, c0, p1),
				Cubic(p0, c0, c1, p1) => draw_cubic(p0, c0, c1, p1),
			};
			mls.push(LineStringGeometry::from(
				points
					.iter()
					.map(|p| {
						[
							8.0 * (p.x * scale + position.x) as f64,
							8.0 * ((height - p.y) * scale + position.y) as f64,
						]
					})
					.collect::<Vec<_>>(),
			));
		}

		let multipolygon = get_multipolygon(mls);

		GeoFeature::new(Geometry::MultiPolygon(multipolygon))
	};

	for (i, c) in text.chars().enumerate() {
		let glyph_id = font.glyph_id(c);
		if let Some(outline) = font.outline(glyph_id) {
			let mut feature = get_char_as_feature(outline, &position);
			feature
				.properties
				.insert(String::from("char"), GeoValue::from(c.to_string()));
			feature.properties.insert(String::from("x"), GeoValue::from(position.x));
			feature.properties.insert(String::from("index"), GeoValue::from(i));
			features.push(feature);
		}
		position.x += scale * font.h_advance_unscaled(glyph_id);
	}

	VectorTileLayer::from_features(String::from(name), features, 4096, 1).unwrap()
}

fn get_multipolygon(mls: MultiLineStringGeometry) -> MultiPolygonGeometry {
	fn get_ring(mut iter: impl Iterator<Item = LineStringGeometry>) -> Option<RingGeometry> {
		let mut points = iter.next()?.into_inner();
		let p0 = points.first()?.clone();

		while points.last()? != &p0 {
			let line = iter.next()?;
			for point in line.into_iter().skip(1) {
				points.push(point);
			}
		}

		Some(RingGeometry(points))
	}

	let mut multipolygon = MultiPolygonGeometry::new();
	let mut iter = mls.into_iter();

	while let Some(ring) = get_ring(&mut iter) {
		if ring.len() == 2 {
			continue;
		}
		if ring.area() > 0.0 {
			multipolygon.push(PolygonGeometry::from(vec![ring]))
		} else {
			multipolygon.last_mut().expect("first ring is missing").push(ring)
		}
	}

	multipolygon
}

#[context("Creating background layer for debug vector tile")]
fn get_background_layer() -> Result<VectorTileLayer> {
	let mut circle = LineStringGeometry::new();
	for i in 0..=100 {
		let a = PI * i as f64 / 50.0;
		circle.push(Coordinates::new((a.cos() + 1.0) * 2047.5, (a.sin() + 1.0) * 2047.5));
	}

	let feature = GeoFeature::new(Geometry::new_line_string(circle));
	VectorTileLayer::from_features(String::from("background"), vec![feature], 4096, 1)
}

fn draw_quad(p0: Point, c0: Point, p1: Point) -> Vec<Point> {
	let mut result: Vec<Point> = vec![p0];
	let devx = p0.x - 2.0 * c0.x + p1.x;
	let devy = p0.y - 2.0 * c0.y + p1.y;
	let devsq = devx * devx + devy * devy;
	if devsq >= 0.333 {
		let tol = 3.0;
		let n = 1 + (tol * devsq).sqrt().sqrt().floor() as usize;
		for i in 1..n {
			let t = (i as f32).div(n as f32);
			result.push(lerp(t, lerp(t, p0, c0), lerp(t, c0, p1)));
		}
	}
	result.push(p1);
	result
}

fn draw_cubic(p0: Point, c0: Point, c1: Point, p1: Point) -> Vec<Point> {
	let mut result: Vec<Point> = vec![p0];
	tessellate_cubic(&mut result, p0, c0, c1, p1, 0);
	return result;

	fn tessellate_cubic(list: &mut Vec<Point>, p0: Point, c0: Point, c1: Point, p1: Point, n: u8) {
		const OBJSPACE_FLATNESS: f32 = 0.35;
		const OBJSPACE_FLATNESS_SQUARED: f32 = OBJSPACE_FLATNESS * OBJSPACE_FLATNESS;
		const MAX_RECURSION_DEPTH: u8 = 16;

		let longlen = distance(p0, c0) + distance(c0, c1) + distance(c1, p1);
		let shortlen = distance(p0, p1);
		let flatness_squared = longlen * longlen - shortlen * shortlen;

		if n < MAX_RECURSION_DEPTH && flatness_squared > OBJSPACE_FLATNESS_SQUARED {
			let p01 = lerp(0.5, p0, c0);
			let p12 = lerp(0.5, c0, c1);
			let p23 = lerp(0.5, c1, p1);

			let pa = lerp(0.5, p01, p12);
			let pb = lerp(0.5, p12, p23);

			let mp = lerp(0.5, pa, pb);

			tessellate_cubic(list, p0, p01, pa, mp, n + 1);
			tessellate_cubic(list, mp, pb, p23, p1, n + 1);
		} else {
			list.push(p1);
		}

		fn distance(p0: Point, p1: Point) -> f32 {
			let dx = p0.x - p1.x;
			let dy = p0.y - p1.y;
			(dx * dx + dy * dy).sqrt()
		}
	}
}

fn lerp(t: f32, p0: Point, p1: Point) -> Point {
	Point {
		x: p0.x + t * (p1.x - p0.x),
		y: p0.y + t * (p1.y - p0.y),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_create_debug_vector_tile() {
		let coord = TileCoord::new(3, 1, 2).unwrap();
		let vt = create_debug_vector_tile(&coord).unwrap();
		assert_eq!(vt.layers.len(), 4);
		assert_eq!(vt.layers[0].features.len(), 1);
		assert_eq!(vt.layers[1].features.len(), 3);
		assert_eq!(vt.layers[2].features.len(), 3);
		assert_eq!(vt.layers[3].features.len(), 3);
	}

	#[test]
	fn test_create_debug_vector_tile_different_coord() {
		let coord = TileCoord::new(14, 6789, 2345).unwrap();
		let vt = create_debug_vector_tile(&coord).unwrap();
		assert_eq!(vt.layers.len(), 4);
		assert_eq!(vt.layers[0].features.len(), 1);
		assert_eq!(vt.layers[1].features.len(), 4);
		assert_eq!(vt.layers[2].features.len(), 6);
		assert_eq!(vt.layers[3].features.len(), 6);
	}

	#[test]
	fn test_draw_quad_straight_line() {
		use ab_glyph::Point;
		let p0 = Point { x: 0.0, y: 0.0 };
		let c0 = Point { x: 0.5, y: 0.5 };
		let p1 = Point { x: 1.0, y: 1.0 };
		let pts = draw_quad(p0, c0, p1);
		assert_eq!(pts.len(), 2, "Expected no subdivision for straight line");
		assert_eq!(pts[0], p0);
		assert_eq!(pts[1], p1);
	}

	#[test]
	fn test_draw_quad_curve() {
		use ab_glyph::Point;
		let p0 = Point { x: 0.0, y: 0.0 };
		let c0 = Point { x: 0.0, y: 1.0 };
		let p1 = Point { x: 1.0, y: 1.0 };
		let pts = draw_quad(p0, c0, p1);
		assert!(pts.len() > 2, "Expected subdivision for curved quad");
		assert_eq!(pts.first().copied(), Some(p0));
		assert_eq!(pts.last().copied(), Some(p1));
	}

	#[test]
	fn test_draw_cubic_straight_line() {
		use ab_glyph::Point;
		let p0 = Point { x: 0.0, y: 0.0 };
		let c0 = Point { x: 0.333, y: 0.333 };
		let c1 = Point { x: 0.666, y: 0.666 };
		let p1 = Point { x: 1.0, y: 1.0 };
		let pts = draw_cubic(p0, c0, c1, p1);
		assert_eq!(pts.len(), 2, "Expected no subdivision for straight cubic");
		assert_eq!(pts[0], p0);
		assert_eq!(pts[1], p1);
	}

	#[test]
	fn test_draw_cubic_curve() {
		use ab_glyph::Point;
		let p0 = Point { x: 0.0, y: 0.0 };
		let c0 = Point { x: 0.0, y: 1.0 };
		let c1 = Point { x: 1.0, y: 0.0 };
		let p1 = Point { x: 1.0, y: 1.0 };
		let pts = draw_cubic(p0, c0, c1, p1);
		assert!(pts.len() > 2, "Expected subdivision for curved cubic");
		assert_eq!(pts.first().copied(), Some(p0));
		assert_eq!(pts.last().copied(), Some(p1));
	}
}
