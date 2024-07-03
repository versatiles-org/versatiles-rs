use crate::geometry::*;

pub fn area_ring(c: &Coordinates1) -> f64 {
	let mut sum = 0f64;
	let mut p2 = c.last().unwrap();
	for p1 in c.iter() {
		sum += (p2[0] - p1[0]) * (p1[1] + p2[1]);
		p2 = p1
	}
	sum
}

pub fn area_polygon(c: &Coordinates2) -> f64 {
	let mut outer = true;
	let mut sum = 0.0;
	for ring in c {
		if outer {
			sum = area_ring(ring);
			outer = false;
		} else {
			sum -= area_ring(ring);
		}
	}
	sum
}

pub fn area_multi_polygon(c: &Coordinates3) -> f64 {
	let mut sum = 0.0;
	for polygon in c {
		sum += area_polygon(polygon);
	}
	sum
}
