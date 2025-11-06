//! This module defines the `TileOutline` utility for aggregating tile bounding boxes into unified polygonal outlines.
//! It can collect multiple tile or geographic bounding boxes, merge them into a single `MultiPolygon`,
//! and export them as `GeoFeature` objects suitable for GeoJSON serialization.

use crate::geo::GeoFeature;
use geo::{MultiPolygon, Polygon, unary_union};
use versatiles_core::{GeoBBox, TileBBox, TileCoord};

/// Represents a collection of tile or geographic bounding boxes that can be merged into a unified polygon outline.
///
/// Used for visualizing or exporting the outline of a set of map tiles.
/// Internally stores polygons and merges them via geometric union operations.
#[derive(Debug, Clone, Default)]
pub struct TileOutline {
	polygons: Vec<geo::Polygon<f64>>,
}

impl TileOutline {
	/// Creates an empty `TileOutline` with no polygons.
	#[must_use]
	pub fn new() -> Self {
		Self { polygons: Vec::new() }
	}

	/// Adds an arbitrary polygon to the outline.
	pub fn add_polygon(&mut self, polygon: Polygon<f64>) {
		self.polygons.push(polygon);
	}

	/// Adds a rectangular polygon corresponding to a given geographic bounding box (`GeoBBox`).
	///
	/// Converts the bounding box corners into a closed ring polygon.
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

	/// Adds a bounding box defined in tile coordinates (`TileBBox`) if it can be converted to a geographic bounding box.
	pub fn add_tile_bbox(&mut self, bbox: TileBBox) {
		if let Some(bbox) = bbox.to_geo_bbox() {
			self.add_geo_bbox(&bbox);
		}
	}

	/// Adds a tile coordinate (`TileCoord`) by converting it into its corresponding geographic bounding box.
	pub fn add_coord(&mut self, coord: TileCoord) {
		self.add_geo_bbox(&coord.to_geo_bbox());
	}

	/// Returns a [`geo::MultiPolygon`] representing the unified outline of all polygons added.
	///
	/// Uses a geometric union to merge overlapping or adjacent polygons.
	#[must_use]
	pub fn to_multi_polygon(&self) -> MultiPolygon<f64> {
		unary_union(&self.polygons)
	}

	/// Converts the outline into a [`GeoFeature`] suitable for GeoJSON serialization.
	///
	/// The resulting feature contains a single `Polygon` or `MultiPolygon` geometry depending on the data.
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
			"{\"geometry\":{\"coordinates\":[[[-180,85.051129],[-180,-0],[0,-0],[0,85.051129],[-180,85.051129]]],\"type\":\"Polygon\"},\"properties\":{},\"type\":\"Feature\"}"
		);
	}
}
