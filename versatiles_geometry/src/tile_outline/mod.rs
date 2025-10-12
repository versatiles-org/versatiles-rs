use crate::geo::GeoFeature;
use geo::{MultiPolygon, Polygon, unary_union};
use versatiles_core::{GeoBBox, TileBBox, TileCoord};

#[derive(Debug, Clone, Default)]
pub struct TileOutline {
	polygons: Vec<geo::Polygon<f64>>,
}

impl TileOutline {
	#[must_use]
	pub fn new() -> Self {
		Self { polygons: Vec::new() }
	}

	pub fn add_polygon(&mut self, polygon: Polygon<f64>) {
		self.polygons.push(polygon);
	}

	pub fn add_geo_bbox(&mut self, bbox: &GeoBBox) {
		self.add_polygon(Polygon::new(
			geo::LineString::from(vec![
				(bbox.0, bbox.1),
				(bbox.2, bbox.1),
				(bbox.2, bbox.3),
				(bbox.0, bbox.3),
				(bbox.0, bbox.1),
			]),
			vec![],
		));
	}

	pub fn add_tile_bbox(&mut self, bbox: TileBBox) {
		self.add_geo_bbox(&bbox.to_geo_bbox());
	}

	pub fn add_coord(&mut self, coord: TileCoord) {
		self.add_geo_bbox(&coord.to_geo_bbox());
	}

	#[must_use]
	pub fn to_multi_polygon(&self) -> MultiPolygon<f64> {
		unary_union(&self.polygons)
	}

	#[must_use]
	pub fn to_json_string(&self) -> String {
		let multi_polygon = self.to_multi_polygon();
		let obj = GeoFeature::from(multi_polygon).to_json();
		obj.stringify()
	}
}
