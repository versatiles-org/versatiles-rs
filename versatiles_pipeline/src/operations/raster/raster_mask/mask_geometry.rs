//! Mask geometry processing for raster mask operations.
//!
//! This module provides geometry handling for applying polygon masks to raster tiles,
//! including:
//! - Loading and parsing GeoJSON files
//! - Converting WGS84 coordinates to Web Mercator
//! - Building R-tree spatial indices for efficient distance queries
//! - Computing signed distances and alpha values for pixel masking

use super::blur_function::BlurFunction;
use anyhow::{Result, ensure};
use rstar::{AABB, RTree, RTreeObject};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use versatiles_derive::context;
use versatiles_geometry::geo::{Geometry, GeometryTrait, MultiPolygonGeometry, PolygonGeometry, RingGeometry};
use versatiles_geometry::geojson::read_geojson;

/// Classification of a tile's relationship to the mask geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileClassification {
	/// Tile is entirely within the mask - pass through unchanged
	FullyInside,
	/// Tile is entirely outside the mask - skip tile (filter from stream)
	FullyOutside,
	/// Tile overlaps the mask edge - compute per-pixel alpha
	Partial,
}

/// A line segment used for R-tree spatial indexing.
#[derive(Clone, Debug)]
pub struct EdgeSegment {
	/// Start point of the segment [x, y] in Mercator meters
	pub start: [f64; 2],
	/// End point of the segment [x, y] in Mercator meters
	pub end: [f64; 2],
}

impl EdgeSegment {
	/// Create a new edge segment.
	#[must_use]
	pub fn new(start: [f64; 2], end: [f64; 2]) -> Self {
		Self { start, end }
	}

	/// Compute the squared distance from a point to this line segment.
	#[must_use]
	pub fn distance_squared_to_point(&self, point: [f64; 2]) -> f64 {
		let [ax, ay] = self.start;
		let [bx, by] = self.end;
		let [px, py] = point;

		let abx = bx - ax;
		let aby = by - ay;
		let apx = px - ax;
		let apy = py - ay;

		let ab_sq = abx * abx + aby * aby;

		if ab_sq == 0.0 {
			// Degenerate segment (single point)
			return apx * apx + apy * apy;
		}

		// Project point onto line, clamped to segment
		let t = ((apx * abx + apy * aby) / ab_sq).clamp(0.0, 1.0);

		// Closest point on segment
		let cx = ax + t * abx;
		let cy = ay + t * aby;

		// Distance squared
		let dx = px - cx;
		let dy = py - cy;
		dx * dx + dy * dy
	}

	/// Check if a horizontal ray from point (px, py) going to +∞ crosses this edge.
	/// Uses the ray casting algorithm logic for point-in-polygon testing.
	#[must_use]
	pub fn ray_crosses(&self, px: f64, py: f64) -> bool {
		let [x1, y1] = self.start;
		let [x2, y2] = self.end;

		// Check if the ray at height py could intersect this edge
		// The edge must span the y coordinate (one endpoint above, one below or on)
		if (y1 > py) == (y2 > py) {
			return false;
		}

		// Calculate x coordinate where edge crosses the horizontal line y = py
		let x_intersect = x1 + (x2 - x1) * (py - y1) / (y2 - y1);

		// Ray crosses if intersection is to the right of point
		px < x_intersect
	}
}

impl RTreeObject for EdgeSegment {
	type Envelope = AABB<[f64; 2]>;

	fn envelope(&self) -> Self::Envelope {
		AABB::from_corners(
			[self.start[0].min(self.end[0]), self.start[1].min(self.end[1])],
			[self.start[0].max(self.end[0]), self.start[1].max(self.end[1])],
		)
	}
}

/// Pre-computed mask geometry with spatial index for efficient distance queries.
pub struct MaskGeometry {
	/// Geometry in Web Mercator (EPSG:3857) for meter-based calculations
	#[allow(dead_code)]
	polygon_mercator: MultiPolygonGeometry,

	/// R-tree index of polygon edges for fast distance queries
	edge_rtree: RTree<EdgeSegment>,

	/// Bounding box in Mercator meters [x_min, y_min, x_max, y_max]
	bounds_mercator: [f64; 4],

	/// Buffer distance in meters (positive = expand, negative = shrink)
	buffer_meters: f64,

	/// Blur distance in meters for edge softening
	blur_meters: f64,

	/// Blur interpolation function
	blur_function: BlurFunction,

	/// Effective outer threshold: buffer + blur (for tile classification)
	outer_threshold: f64,
}

impl std::fmt::Debug for MaskGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MaskGeometry")
			.field("bounds_mercator", &self.bounds_mercator)
			.field("buffer_meters", &self.buffer_meters)
			.field("blur_meters", &self.blur_meters)
			.field("blur_function", &self.blur_function)
			.field("outer_threshold", &self.outer_threshold)
			.field("edge_count", &self.edge_rtree.size())
			.finish()
	}
}

impl MaskGeometry {
	/// Load mask geometry from a GeoJSON file.
	///
	/// # Arguments
	/// * `path` - Path to the GeoJSON file containing Polygon or MultiPolygon geometries
	/// * `buffer_meters` - Buffer distance in meters (positive = expand, negative = shrink)
	/// * `blur_meters` - Blur distance in meters for edge softening
	/// * `blur_function` - Interpolation function for blur
	///
	/// # Errors
	/// Returns an error if the file cannot be read or parsed, or if no valid polygons are found.
	#[context("Loading mask geometry from GeoJSON file")]
	pub fn from_geojson(
		path: impl AsRef<Path>,
		buffer_meters: f64,
		blur_meters: f64,
		blur_function: BlurFunction,
	) -> Result<Self> {
		let path = path.as_ref();
		ensure!(path.exists(), "GeoJSON file not found: {}", path.display());

		let file = File::open(path)?;
		let reader = BufReader::new(file);
		let collection = read_geojson(reader)?;

		// Collect all polygons from the GeoJSON
		let mut polygons_wgs84: Vec<PolygonGeometry> = Vec::new();

		for feature in collection.features {
			match feature.geometry {
				Geometry::Polygon(poly) => {
					polygons_wgs84.push(poly);
				}
				Geometry::MultiPolygon(multi) => {
					for poly in multi.0 {
						polygons_wgs84.push(poly);
					}
				}
				_ => {
					// Skip non-polygon geometries
					log::warn!("Skipping non-polygon geometry in GeoJSON mask file");
				}
			}
		}

		ensure!(
			!polygons_wgs84.is_empty(),
			"No polygon geometries found in GeoJSON file"
		);

		// Convert to Web Mercator
		let multi_polygon = MultiPolygonGeometry(polygons_wgs84).to_mercator();

		// Build R-tree from edges
		let edges = extract_edges(&multi_polygon);
		let edge_rtree = RTree::bulk_load(edges);

		// Compute bounding box
		let bounds_mercator = multi_polygon
			.compute_bounds()
			.ok_or_else(|| anyhow::anyhow!("Mask geometry is empty - no polygons found"))?;

		let blur_meters = blur_meters.max(0.0);
		let outer_threshold = buffer_meters + blur_meters;

		Ok(Self {
			polygon_mercator: multi_polygon,
			edge_rtree,
			bounds_mercator,
			buffer_meters,
			blur_meters,
			blur_function,
			outer_threshold,
		})
	}

	/// Classify a tile based on its relationship to the mask geometry.
	///
	/// # Arguments
	/// * `tile_bbox_mercator` - Tile bounding box in Mercator meters [x_min, y_min, x_max, y_max]
	///
	/// # Returns
	/// Classification indicating whether the tile is fully inside, fully outside, or partial.
	#[must_use]
	pub fn classify_tile(&self, tile_bbox_mercator: [f64; 4]) -> TileClassification {
		let [tx_min, ty_min, tx_max, ty_max] = tile_bbox_mercator;
		let [mx_min, my_min, mx_max, my_max] = self.bounds_mercator;

		// Quick rejection: if tile is far outside the mask bounds (accounting for buffer+blur)
		if tx_max < mx_min - self.outer_threshold
			|| tx_min > mx_max + self.outer_threshold
			|| ty_max < my_min - self.outer_threshold
			|| ty_min > my_max + self.outer_threshold
		{
			return TileClassification::FullyOutside;
		}

		// Check all four corners of the tile
		let corners = [[tx_min, ty_min], [tx_min, ty_max], [tx_max, ty_min], [tx_max, ty_max]];

		let mut all_inside = true;
		let mut all_outside = true;

		for corner in &corners {
			let signed_dist = self.signed_distance(*corner);

			if signed_dist > self.blur_meters {
				// Corner is fully inside (beyond blur zone)
				all_outside = false;
			} else if signed_dist < 0.0 {
				// Corner is fully outside
				all_inside = false;
			} else {
				// Corner is in the blur zone
				all_inside = false;
				all_outside = false;
			}
		}

		// Also check center point for better classification
		let center = [f64::midpoint(tx_min, tx_max), f64::midpoint(ty_min, ty_max)];
		let center_dist = self.signed_distance(center);

		if center_dist < 0.0 {
			all_inside = false;
		} else if center_dist <= self.blur_meters {
			all_inside = false;
			all_outside = false;
		} else {
			all_outside = false;
		}

		if all_inside {
			// Additional check: ensure minimum distance to edge is greater than tile diagonal + blur
			let tile_diagonal = ((tx_max - tx_min).powi(2) + (ty_max - ty_min).powi(2)).sqrt();
			if center_dist > tile_diagonal / 2.0 + self.blur_meters {
				return TileClassification::FullyInside;
			}
		}

		if all_outside {
			// Check if the minimum distance from tile center to polygon is greater than
			// the tile diagonal / 2, meaning the polygon cannot intersect the tile
			let tile_diagonal = ((tx_max - tx_min).powi(2) + (ty_max - ty_min).powi(2)).sqrt();
			if center_dist < -(tile_diagonal / 2.0) {
				return TileClassification::FullyOutside;
			}
		}

		TileClassification::Partial
	}

	/// Compute the signed distance from a point to the mask boundary.
	///
	/// # Arguments
	/// * `point` - Point in Mercator meters [x, y]
	///
	/// # Returns
	/// Signed distance in meters:
	/// - Positive: inside the polygon (adjusted by buffer)
	/// - Negative: outside the polygon (adjusted by buffer)
	#[must_use]
	pub fn signed_distance(&self, point: [f64; 2]) -> f64 {
		// Find distance to nearest edge using R-tree
		let unsigned_dist = self.distance_to_nearest_edge(point);

		// Determine if point is inside polygon using R-tree accelerated ray casting
		let is_inside = self.contains_point_rtree(point[0], point[1]);

		// Apply buffer offset and return signed distance
		// Positive buffer = expand mask = shift threshold outward
		// Negative buffer = shrink mask = shift threshold inward
		let raw_signed = if is_inside { unsigned_dist } else { -unsigned_dist };
		raw_signed + self.buffer_meters
	}

	/// Convert signed distance to alpha value for pixel masking.
	///
	/// # Arguments
	/// * `signed_dist` - Signed distance in meters
	///
	/// # Returns
	/// Alpha value in range [0, 255]
	#[must_use]
	pub fn distance_to_alpha(&self, signed_dist: f64) -> u8 {
		if self.blur_meters <= 0.0 {
			return if signed_dist > 0.0 { 255 } else { 0 };
		}

		// Normalize to [0, 1] over blur range
		// At signed_dist = blur_meters: t = 1.0 (fully inside)
		// At signed_dist = 0: t = 0.5 (halfway)
		// At signed_dist = -blur_meters: t = 0.0 (fully outside, but this is actually beyond blur)
		let t = f64::midpoint(signed_dist / self.blur_meters, 1.0);
		let t = t.clamp(0.0, 1.0);

		let alpha = self.blur_function.interpolate(t);

		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		{
			(alpha * 255.0).round() as u8
		}
	}

	/// Get the mask bounds in Mercator coordinates.
	#[must_use]
	#[allow(dead_code)]
	pub fn bounds_mercator(&self) -> [f64; 4] {
		self.bounds_mercator
	}

	/// Check if a point is inside the polygon using R-tree accelerated ray casting.
	///
	/// This is much faster than the naive O(n) algorithm because it only checks
	/// edges that could potentially intersect with the horizontal ray.
	///
	/// Complexity: O(log n + k) where k is the number of edges at the query y-level
	#[must_use]
	fn contains_point_rtree(&self, x: f64, y: f64) -> bool {
		// Query the R-tree for edges that span the y coordinate
		// We create a thin horizontal envelope at height y extending from -∞ to x
		// But since R-tree needs a bounded envelope, we use the mask bounds
		let [mx_min, _, mx_max, _] = self.bounds_mercator;

		// Create envelope for edges that could intersect a horizontal ray at y
		let envelope = AABB::from_corners([mx_min, y], [mx_max, y]);

		let mut crossings = 0;

		for edge in self.edge_rtree.locate_in_envelope_intersecting(&envelope) {
			// Only count edges that actually span y (the envelope query might include
			// edges that just touch y at their endpoints)
			if edge.ray_crosses(x, y) {
				crossings += 1;
			}
		}

		// Odd number of crossings means inside
		crossings % 2 == 1
	}

	/// Compute alpha values for a tile using hierarchical subdivision.
	///
	/// This method subdivides the tile into a grid of sub-regions, classifies each
	/// as fully inside, fully outside, or partial, and only computes per-pixel
	/// values for partial regions.
	///
	/// # Arguments
	/// * `tile_bbox` - Tile bounding box in Mercator meters [x_min, y_min, x_max, y_max]
	/// * `width` - Tile width in pixels
	/// * `height` - Tile height in pixels
	///
	/// # Returns
	/// A vector of alpha values (0-255) for each pixel, in row-major order.
	#[must_use]
	pub fn compute_alpha_grid(&self, tile_bbox: [f64; 4], width: u32, height: u32) -> Vec<u8> {
		let [x_min, y_min, x_max, y_max] = tile_bbox;
		let px_width = (x_max - x_min) / f64::from(width);
		let px_height = (y_max - y_min) / f64::from(height);

		let mut alpha = vec![0u8; (width * height) as usize];

		// Subdivide into grid of sub-regions (e.g., 8x8 for 256x256 tiles = 32x32 pixel blocks)
		let grid_size: u32 = 8;
		let block_width = width / grid_size;
		let block_height = height / grid_size;

		for grid_y in 0..grid_size {
			for grid_x in 0..grid_size {
				let px_start_x = grid_x * block_width;
				let px_start_y = grid_y * block_height;
				let px_end_x = if grid_x == grid_size - 1 {
					width
				} else {
					px_start_x + block_width
				};
				let px_end_y = if grid_y == grid_size - 1 {
					height
				} else {
					px_start_y + block_height
				};

				// Compute sub-region bbox in Mercator coordinates
				let sub_x_min = x_min + f64::from(px_start_x) * px_width;
				let sub_x_max = x_min + f64::from(px_end_x) * px_width;
				let sub_y_max = y_max - f64::from(px_start_y) * px_height;
				let sub_y_min = y_max - f64::from(px_end_y) * px_height;

				let sub_bbox = [sub_x_min, sub_y_min, sub_x_max, sub_y_max];
				let classification = self.classify_sub_region(sub_bbox);

				match classification {
					TileClassification::FullyInside => {
						// Fill entire sub-region with opaque
						for py in px_start_y..px_end_y {
							for px in px_start_x..px_end_x {
								alpha[(py * width + px) as usize] = 255;
							}
						}
					}
					TileClassification::FullyOutside => {
						// Already 0, nothing to do
					}
					TileClassification::Partial => {
						// Compute per-pixel for this sub-region
						for py in px_start_y..px_end_y {
							for px in px_start_x..px_end_x {
								let merc_x = x_min + (f64::from(px) + 0.5) * px_width;
								let merc_y = y_max - (f64::from(py) + 0.5) * px_height;

								let signed_dist = self.signed_distance([merc_x, merc_y]);
								alpha[(py * width + px) as usize] = self.distance_to_alpha(signed_dist);
							}
						}
					}
				}
			}
		}

		alpha
	}

	/// Classify a sub-region of a tile for hierarchical processing.
	///
	/// Similar to `classify_tile` but optimized for smaller regions within a tile.
	fn classify_sub_region(&self, bbox: [f64; 4]) -> TileClassification {
		let [sx_min, sy_min, sx_max, sy_max] = bbox;

		// Quick rejection against mask bounds
		let [mx_min, my_min, mx_max, my_max] = self.bounds_mercator;
		if sx_max < mx_min - self.outer_threshold
			|| sx_min > mx_max + self.outer_threshold
			|| sy_max < my_min - self.outer_threshold
			|| sy_min > my_max + self.outer_threshold
		{
			return TileClassification::FullyOutside;
		}

		// Check corners and center
		let corners = [[sx_min, sy_min], [sx_min, sy_max], [sx_max, sy_min], [sx_max, sy_max]];
		let center = [f64::midpoint(sx_min, sx_max), f64::midpoint(sy_min, sy_max)];

		// Compute signed distances for all sample points
		let corner_dists: Vec<f64> = corners.iter().map(|&c| self.signed_distance(c)).collect();
		let center_dist = self.signed_distance(center);

		// Check if all points are clearly inside (beyond blur zone)
		let all_inside = corner_dists.iter().all(|&d| d > self.blur_meters) && center_dist > self.blur_meters;

		// Check if all points are clearly outside
		let all_outside = corner_dists.iter().all(|&d| d < 0.0) && center_dist < 0.0;

		if all_inside {
			// Additional check: ensure the sub-region doesn't cross any edge
			let diagonal = ((sx_max - sx_min).powi(2) + (sy_max - sy_min).powi(2)).sqrt();
			if center_dist > diagonal / 2.0 + self.blur_meters {
				return TileClassification::FullyInside;
			}
		}

		if all_outside {
			let diagonal = ((sx_max - sx_min).powi(2) + (sy_max - sy_min).powi(2)).sqrt();
			if center_dist < -(diagonal / 2.0) {
				return TileClassification::FullyOutside;
			}
		}

		TileClassification::Partial
	}

	/// Find the distance to the nearest edge using the R-tree.
	#[allow(clippy::float_cmp)]
	fn distance_to_nearest_edge(&self, point: [f64; 2]) -> f64 {
		// Use the R-tree to find nearby edges efficiently
		let search_radius = self.outer_threshold.max(1000.0); // At least 1km search radius
		let envelope = AABB::from_corners(
			[point[0] - search_radius, point[1] - search_radius],
			[point[0] + search_radius, point[1] + search_radius],
		);

		let mut min_dist_sq = f64::MAX;

		for edge in self.edge_rtree.locate_in_envelope_intersecting(&envelope) {
			let dist_sq = edge.distance_squared_to_point(point);
			if dist_sq < min_dist_sq {
				min_dist_sq = dist_sq;
			}
		}

		// If no edges found in search radius, do a full search
		if min_dist_sq == f64::MAX {
			for edge in &self.edge_rtree {
				let dist_sq = edge.distance_squared_to_point(point);
				if dist_sq < min_dist_sq {
					min_dist_sq = dist_sq;
				}
			}
		}

		min_dist_sq.sqrt()
	}
}

/// Extract all edges from a MultiPolygon as EdgeSegments.
fn extract_edges(multi_poly: &MultiPolygonGeometry) -> Vec<EdgeSegment> {
	let mut edges = Vec::new();

	for poly in &multi_poly.0 {
		for ring in &poly.0 {
			extract_ring_edges(ring, &mut edges);
		}
	}

	edges
}

/// Extract edges from a ring and add them to the edges vector.
fn extract_ring_edges(ring: &RingGeometry, edges: &mut Vec<EdgeSegment>) {
	let coords = &ring.0;

	for window in coords.windows(2) {
		if let [c1, c2] = window {
			edges.push(EdgeSegment::new([c1.x(), c1.y()], [c2.x(), c2.y()]));
		}
	}
}

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
	use super::*;

	#[test]
	fn test_edge_segment_distance() {
		let edge = EdgeSegment::new([0.0, 0.0], [10.0, 0.0]);

		// Point on the segment
		let dist = edge.distance_squared_to_point([5.0, 0.0]);
		assert!((dist - 0.0).abs() < 1e-10);

		// Point perpendicular to segment
		let dist = edge.distance_squared_to_point([5.0, 3.0]).sqrt();
		assert!((dist - 3.0).abs() < 1e-10);

		// Point beyond segment end
		let dist = edge.distance_squared_to_point([15.0, 0.0]).sqrt();
		assert!((dist - 5.0).abs() < 1e-10);
	}

	#[test]
	fn test_blur_function_alpha() {
		// Create a simple test case
		let blur_func = BlurFunction::Linear;

		// Test with no blur
		let mask = MaskGeometry {
			polygon_mercator: MultiPolygonGeometry(vec![]),
			edge_rtree: RTree::new(),
			bounds_mercator: [0.0, 0.0, 0.0, 0.0],
			buffer_meters: 0.0,
			blur_meters: 0.0,
			blur_function: blur_func,
			outer_threshold: 0.0,
		};

		assert_eq!(mask.distance_to_alpha(1.0), 255);
		assert_eq!(mask.distance_to_alpha(-1.0), 0);
		assert_eq!(mask.distance_to_alpha(0.0), 0); // Zero is considered outside
	}

	#[test]
	fn test_blur_function_alpha_with_blur() {
		let mask = MaskGeometry {
			polygon_mercator: MultiPolygonGeometry(vec![]),
			edge_rtree: RTree::new(),
			bounds_mercator: [0.0, 0.0, 0.0, 0.0],
			buffer_meters: 0.0,
			blur_meters: 100.0,
			blur_function: BlurFunction::Linear,
			outer_threshold: 100.0,
		};

		// At blur distance, should be fully opaque
		assert_eq!(mask.distance_to_alpha(100.0), 255);

		// At edge (0 distance), should be 50%
		let alpha = mask.distance_to_alpha(0.0);
		assert!((alpha as f64 - 127.5).abs() < 1.5);

		// Beyond blur on outside, should be transparent
		assert_eq!(mask.distance_to_alpha(-100.0), 0);
	}

	#[test]
	fn test_ray_crosses() {
		// Horizontal edge from (0, 5) to (10, 5)
		let edge = EdgeSegment::new([0.0, 5.0], [10.0, 5.0]);

		// Ray at y=5 is exactly on the edge - behavior depends on implementation
		// Ray at y=4 or y=6 should not cross (both endpoints on same side)
		assert!(!edge.ray_crosses(5.0, 4.0));
		assert!(!edge.ray_crosses(5.0, 6.0));

		// Vertical edge from (5, 0) to (5, 10)
		let edge = EdgeSegment::new([5.0, 0.0], [5.0, 10.0]);

		// Point at (3, 5) - ray goes right, should cross edge at x=5
		assert!(edge.ray_crosses(3.0, 5.0));

		// Point at (7, 5) - ray goes right, edge is to the left, should not cross
		assert!(!edge.ray_crosses(7.0, 5.0));

		// Point at (3, 15) - ray goes right but y=15 is outside edge's y range
		assert!(!edge.ray_crosses(3.0, 15.0));
	}

	#[test]
	fn test_contains_point_rtree() {
		use versatiles_geometry::geo::{Coordinates, PolygonGeometry, RingGeometry};

		// Create a simple square polygon: (0,0) to (100,100)
		let ring = RingGeometry(vec![
			Coordinates::new(0.0, 0.0),
			Coordinates::new(100.0, 0.0),
			Coordinates::new(100.0, 100.0),
			Coordinates::new(0.0, 100.0),
			Coordinates::new(0.0, 0.0),
		]);
		let poly = PolygonGeometry(vec![ring]);
		let multi = MultiPolygonGeometry(vec![poly]);

		let edges = extract_edges(&multi);
		let edge_rtree = RTree::bulk_load(edges);

		let mask = MaskGeometry {
			polygon_mercator: multi,
			edge_rtree,
			bounds_mercator: [0.0, 0.0, 100.0, 100.0],
			buffer_meters: 0.0,
			blur_meters: 0.0,
			blur_function: BlurFunction::Linear,
			outer_threshold: 0.0,
		};

		// Point inside
		assert!(mask.contains_point_rtree(50.0, 50.0));
		assert!(mask.contains_point_rtree(10.0, 10.0));
		assert!(mask.contains_point_rtree(90.0, 90.0));

		// Point outside
		assert!(!mask.contains_point_rtree(-10.0, 50.0));
		assert!(!mask.contains_point_rtree(110.0, 50.0));
		assert!(!mask.contains_point_rtree(50.0, -10.0));
		assert!(!mask.contains_point_rtree(50.0, 110.0));
	}

	#[test]
	fn test_compute_alpha_grid_fully_inside() {
		use versatiles_geometry::geo::{Coordinates, PolygonGeometry, RingGeometry};

		// Create a large polygon that fully contains the test tile
		let ring = RingGeometry(vec![
			Coordinates::new(-1000.0, -1000.0),
			Coordinates::new(1000.0, -1000.0),
			Coordinates::new(1000.0, 1000.0),
			Coordinates::new(-1000.0, 1000.0),
			Coordinates::new(-1000.0, -1000.0),
		]);
		let poly = PolygonGeometry(vec![ring]);
		let multi = MultiPolygonGeometry(vec![poly]);

		let edges = extract_edges(&multi);
		let edge_rtree = RTree::bulk_load(edges);

		let mask = MaskGeometry {
			polygon_mercator: multi,
			edge_rtree,
			bounds_mercator: [-1000.0, -1000.0, 1000.0, 1000.0],
			buffer_meters: 0.0,
			blur_meters: 0.0,
			blur_function: BlurFunction::Linear,
			outer_threshold: 0.0,
		};

		// Tile fully inside the polygon
		let alpha = mask.compute_alpha_grid([0.0, 0.0, 100.0, 100.0], 16, 16);

		// All pixels should be opaque
		assert!(alpha.iter().all(|&a| a == 255));
	}

	#[test]
	fn test_compute_alpha_grid_fully_outside() {
		use versatiles_geometry::geo::{Coordinates, PolygonGeometry, RingGeometry};

		// Create a polygon far from the test tile
		let ring = RingGeometry(vec![
			Coordinates::new(10000.0, 10000.0),
			Coordinates::new(11000.0, 10000.0),
			Coordinates::new(11000.0, 11000.0),
			Coordinates::new(10000.0, 11000.0),
			Coordinates::new(10000.0, 10000.0),
		]);
		let poly = PolygonGeometry(vec![ring]);
		let multi = MultiPolygonGeometry(vec![poly]);

		let edges = extract_edges(&multi);
		let edge_rtree = RTree::bulk_load(edges);

		let mask = MaskGeometry {
			polygon_mercator: multi,
			edge_rtree,
			bounds_mercator: [10000.0, 10000.0, 11000.0, 11000.0],
			buffer_meters: 0.0,
			blur_meters: 0.0,
			blur_function: BlurFunction::Linear,
			outer_threshold: 0.0,
		};

		// Tile fully outside the polygon
		let alpha = mask.compute_alpha_grid([0.0, 0.0, 100.0, 100.0], 16, 16);

		// All pixels should be transparent
		assert!(alpha.iter().all(|&a| a == 0));
	}
}
