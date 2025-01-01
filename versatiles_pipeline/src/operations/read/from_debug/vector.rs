use ab_glyph::{Font, FontArc, Outline, OutlineCurve::*, Point};
use anyhow::Result;
use lazy_static::lazy_static;
use std::{f64::consts::PI, ops::Div, vec};
use versatiles_core::types::{Blob, TileCoord3};
use versatiles_geometry::{
	math,
	vector_tile::{VectorTile, VectorTileLayer},
	Coordinates1, Coordinates2, Coordinates3, GeoFeature, Geometry, MultiPolygonGeometry,
};

lazy_static! {
	static ref FONT: FontArc = FontArc::try_from_slice(include_bytes!("./trim.ttf")).unwrap();
}

pub fn create_debug_vector_tile(coord: &TileCoord3) -> Result<Blob> {
	let tile = VectorTile::new(vec![
		get_background_layer()?,
		draw_text("debug_z", 140.0, format!("z:{}", coord.z)),
		draw_text("debug_x", 190.0, format!("x:{}", coord.x)),
		draw_text("debug_y", 240.0, format!("y:{}", coord.y)),
	]);

	tile.to_blob()
}

fn draw_text(name: &str, y: f32, text: String) -> VectorTileLayer {
	let font: &FontArc = &FONT;

	let mut features: Vec<GeoFeature> = Vec::new();
	let height = font.height_unscaled();
	let scale: f32 = 80.0 / height;

	let mut position = Point { x: 100.0, y };

	let get_char_as_feature = |outline: Outline, position: &Point| {
		let mut multilinestring = Coordinates2::new();
		for curve in outline.curves {
			let points = match curve {
				Line(p0, p1) => vec![p0, p1],
				Quad(p0, c0, p1) => draw_quad(p0, c0, p1),
				Cubic(p0, c0, c1, p1) => draw_cubic(p0, c0, c1, p1),
			};
			multilinestring.push(Vec::from_iter(points.iter().map(|p| {
				[
					8.0 * (p.x * scale + position.x) as f64,
					8.0 * ((height - p.y) * scale + position.y) as f64,
				]
			})));
		}

		let multipolygon = get_multipolygon(multilinestring);

		GeoFeature::new(Geometry::MultiPolygon(multipolygon))
	};

	for c in text.chars() {
		let glyph_id = font.glyph_id(c);
		if let Some(outline) = font.outline(glyph_id) {
			features.push(get_char_as_feature(outline, &position));
		}
		position.x += scale * font.h_advance_unscaled(glyph_id);
	}

	VectorTileLayer::from_features(String::from(name), features, 4096, 1).unwrap()
}

fn get_multipolygon(multilinestring: Coordinates2) -> MultiPolygonGeometry {
	fn get_ring(mut iter: impl Iterator<Item = Coordinates1>) -> Option<Coordinates1> {
		let mut ring = iter.next()?;
		let p0 = *ring.first().unwrap();
		while ring.last().unwrap() != &p0 {
			let line = iter.next().unwrap();
			for point in line.into_iter().skip(1) {
				ring.push(point);
			}
		}
		Some(ring)
	}

	let mut multipolygon = Coordinates3::new();
	let mut iter = multilinestring.into_iter();

	while let Some(ring) = get_ring(&mut iter) {
		if ring.len() == 2 {
			continue;
		}
		if math::area_ring(&ring) > 0.0 {
			multipolygon.push(vec![ring])
		} else {
			multipolygon.last_mut().expect("first ring is missing").push(ring)
		}
	}

	MultiPolygonGeometry::new(multipolygon)
}

fn get_background_layer() -> Result<VectorTileLayer> {
	let mut circle: Coordinates1 = vec![];
	for i in 0..=100 {
		let a = PI * i as f64 / 50.0;
		circle.push([(a.cos() + 1.0) * 2047.5, (a.sin() + 1.0) * 2047.5])
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
		let coord = TileCoord3::new(1, 2, 3).unwrap();
		let blob = create_debug_vector_tile(&coord).unwrap();
		let vt = VectorTile::from_blob(&blob).unwrap();
		assert_eq!(vt.layers.len(), 4);
		assert_eq!(vt.layers[0].features.len(), 1);
		assert_eq!(vt.layers[1].features.len(), 3);
		assert_eq!(vt.layers[2].features.len(), 3);
		assert_eq!(vt.layers[3].features.len(), 3);
	}

	#[test]
	fn test_create_debug_vector_tile_different_coord() {
		let coord = TileCoord3::new(6789, 2345, 10).unwrap();
		let blob = create_debug_vector_tile(&coord).unwrap();
		let vt = VectorTile::from_blob(&blob).unwrap();
		assert_eq!(vt.layers.len(), 4);
		assert_eq!(vt.layers[0].features.len(), 1);
		assert_eq!(vt.layers[1].features.len(), 4);
		assert_eq!(vt.layers[2].features.len(), 6);
		assert_eq!(vt.layers[3].features.len(), 6);
	}
}
