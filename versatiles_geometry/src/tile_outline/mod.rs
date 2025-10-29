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
				(bbox.x_min, bbox.y_min),
				(bbox.x_max, bbox.y_min),
				(bbox.x_max, bbox.y_max),
				(bbox.x_min, bbox.y_max),
				(bbox.x_min, bbox.y_min),
			]),
			vec![],
		));
	}

	pub fn add_tile_bbox(&mut self, bbox: TileBBox) {
		if let Some(bbox) = bbox.to_geo_bbox() {
			self.add_geo_bbox(&bbox);
		}
	}

	pub fn add_coord(&mut self, coord: TileCoord) {
		self.add_geo_bbox(&coord.to_geo_bbox());
	}

	#[must_use]
	pub fn to_multi_polygon(&self) -> MultiPolygon<f64> {
		unary_union(&self.polygons)
	}

	#[must_use]
	pub fn to_feature(&self) -> GeoFeature {
		let multi_polygon = self.to_multi_polygon();
		let mut feature = GeoFeature::from(multi_polygon);
		feature.to_single_geometry();
		feature
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_outline_is_empty_multipolygon() {
		let outline = TileOutline::new();
		let mp = outline.to_multi_polygon();
		assert_eq!(mp.0.len(), 0, "empty outline should yield empty MultiPolygon");
	}

	#[test]
	fn adjacent_coords_merge_into_single_polygon() {
		// Two adjacent tiles at z=1 along x-direction: (1,0,0) and (1,1,0)
		let mut outline = TileOutline::new();
		outline.add_coord(TileCoord::new(1, 0, 0).unwrap());
		outline.add_coord(TileCoord::new(1, 1, 0).unwrap());
		let mp = outline.to_multi_polygon();
		assert_eq!(mp.0.len(), 1, "adjacent tiles should unify into one polygon");
	}

	#[test]
	fn add_tile_bbox_and_coord_do_not_duplicate() {
		let coord = TileCoord::new(2, 1, 1).unwrap();
		let mut outline = TileOutline::new();
		// Add once via coord
		outline.add_coord(coord);
		// Add again via its TileBBox (same area)
		let bbox = coord.as_tile_bbox();
		outline.add_tile_bbox(bbox);
		let mp = outline.to_multi_polygon();
		assert_eq!(
			mp.0.len(),
			1,
			"same area added twice should still result in a single polygon after union"
		);
	}

	#[test]
	fn json_is_geojson_feature_with_multipolygon() {
		let mut outline = TileOutline::new();
		outline.add_coord(TileCoord::new(1, 0, 0).unwrap());
		let json = outline.to_feature().to_json(Some(6)).stringify();
		assert_eq!(
			json,
			r#"{"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[1.0,0.0],[1.0,1.0],[2.0,1.0],[2.0,0.0],[1.0,0.0]]]]}},"properties":{}}"#
		);
	}
}
