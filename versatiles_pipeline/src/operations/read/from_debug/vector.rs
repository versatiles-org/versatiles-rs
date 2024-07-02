use ab_glyph::{Font, FontArc, Outline, OutlineCurve::*, Point};
use anyhow::Result;
use std::{f64::consts::PI, ops::Div, vec};
use versatiles_core::types::{Blob, TileCoord3};
use versatiles_geometry::{
	vector_tile::{VectorTile, VectorTileLayer},
	AreaTrait, Feature, Geometry, LineStringGeometry, MultiLineStringGeometry, MultiPolygonGeometry,
	PointGeometry, RingGeometry,
};

static mut FONT: Option<FontArc> = None;

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
	let font = unsafe {
		if FONT.is_none() {
			FONT.insert(FontArc::try_from_slice(include_bytes!("./trim.ttf")).unwrap())
		} else {
			FONT.as_ref().unwrap()
		}
	};

	let mut features: Vec<Feature> = Vec::new();
	let height = font.height_unscaled();
	let scale: f32 = 80.0 / height;

	let mut position = Point { x: 100.0, y };

	let get_char_as_feature = |outline: Outline, position: &Point| {
		let mut multi = MultiLineStringGeometry::new();
		for curve in outline.curves {
			let points = match curve {
				Line(p0, p1) => vec![p0, p1],
				Quad(p0, c0, p1) => draw_quad(p0, c0, p1),
				Cubic(p0, c0, c1, p1) => draw_cubic(p0, c0, c1, p1),
			};
			multi.push(
				points
					.iter()
					.map(|p| {
						PointGeometry::new(
							8.0 * (p.x * scale + position.x) as f64,
							8.0 * ((height - p.y) * scale + position.y) as f64,
						)
					})
					.collect::<LineStringGeometry>(),
			);
		}

		let multipolygon = get_multipolygon(multi.into_iter());

		Feature::new(Geometry::MultiPolygon(multipolygon))
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

fn get_multipolygon(mut iter: impl Iterator<Item = LineStringGeometry>) -> MultiPolygonGeometry {
	fn get_ring(mut iter: impl Iterator<Item = LineStringGeometry>) -> Option<RingGeometry> {
		let mut ring: RingGeometry = iter.next()?;
		let p0 = ring.first().unwrap().clone();
		while ring.last().unwrap() != &p0 {
			let line = iter.next().unwrap();
			for point in line.into_iter().skip(1) {
				ring.push(point);
			}
		}
		Some(ring)
	}

	let mut multipolygon = MultiPolygonGeometry::new();

	while let Some(ring) = get_ring(&mut iter) {
		if ring.len() == 2 {
			continue;
		}
		if ring.area() > 0.0 {
			multipolygon.push(vec![ring])
		} else {
			multipolygon
				.last_mut()
				.expect("first ring is missing")
				.push(ring)
		}
	}

	multipolygon
}

fn get_background_layer() -> Result<VectorTileLayer> {
	let mut circle: LineStringGeometry = vec![];
	for i in 0..=100 {
		let a = PI * i as f64 / 50.0;
		circle.push(PointGeometry {
			x: (a.cos() + 1.0) * 2047.5,
			y: (a.sin() + 1.0) * 2047.5,
		})
	}

	let feature = Feature::new(Geometry::LineString(circle));
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

pub fn draw_cubic(p0: Point, c0: Point, c1: Point, p1: Point) -> Vec<Point> {
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
