//! Mask geometry processing for raster mask operations.
//!
//! This module provides geometry handling for applying polygon masks to raster tiles,
//! including:
//! - Loading and parsing GeoJSON files
//! - Converting WGS84 coordinates to Web Mercator
//! - Building R-tree spatial indices for efficient distance queries
//! - Computing signed distances and alpha values for pixel masking

use std::{fs::File, io::BufReader, ops::Range, path::Path};

use anyhow::{Result, ensure};
use geo::BoundingRect;
use geo_types::{Geometry, LineString, MultiPolygon, Polygon};
use rstar::{AABB, RTree, RTreeObject};
use versatiles_core::{EARTH_RADIUS, GeoBBox};
use versatiles_derive::context;
use versatiles_geometry::{
	ext::{MercatorExt, ensure_wgs84_degrees},
	geojson::read_geojson,
};

use super::blur_function::BlurFunction;

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

	/// Where this edge crosses the horizontal line `y = py`, if it crosses at all.
	///
	/// The span test is half-open — an endpoint exactly on the line counts for one
	/// of the two edges meeting there, never both — which is what keeps a shared
	/// vertex from being double-counted by parity.
	#[must_use]
	pub fn crossing_x(&self, py: f64) -> Option<f64> {
		let [x1, y1] = self.start;
		let [x2, y2] = self.end;

		// The edge must span the y coordinate (one endpoint above, one below or on)
		if (y1 > py) == (y2 > py) {
			return None;
		}

		Some(x1 + (x2 - x1) * (py - y1) / (y2 - y1))
	}

	/// Check if a horizontal ray from point (px, py) going to +∞ crosses this edge.
	/// Uses the ray casting algorithm logic for point-in-polygon testing.
	#[must_use]
	pub fn ray_crosses(&self, px: f64, py: f64) -> bool {
		// Ray crosses if the intersection is to the right of the point.
		self.crossing_x(py).is_some_and(|x_intersect| px < x_intersect)
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
	polygon_mercator: MultiPolygon<f64>,

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
		let mut polygons_wgs84: Vec<Polygon<f64>> = Vec::new();

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

		// Convert to Web Mercator, but only once the coordinates have been shown to
		// be lon/lat degrees. Projecting first would clamp a northing in metres to
		// the north edge of the world, and the mask would come out as a shape
		// nobody drew — either a hard error four layers down in the pyramid clip,
		// or, for a mask whose numbers happen to be in range, silently the wrong
		// part of the planet.
		let multi_polygon = MultiPolygon(polygons_wgs84);
		ensure_wgs84_degrees(multi_polygon.bounding_rect())?;
		let multi_polygon = multi_polygon.to_mercator();

		// Build R-tree from edges
		let edges = extract_edges(&multi_polygon);
		let edge_rtree = RTree::bulk_load(edges);

		// Compute bounding box
		let rect = multi_polygon
			.bounding_rect()
			.ok_or_else(|| anyhow::anyhow!("Mask geometry is empty - no polygons found"))?;
		let bounds_mercator = [rect.min().x, rect.min().y, rect.max().x, rect.max().y];

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

		// The blur ramp is centred on the boundary and reaches `blur` metres to *both*
		// sides of it: a pixel is fully opaque at a signed distance of `+blur` and only
		// fully transparent at `-blur`. Both tests below therefore have to clear the
		// whole band; testing the outside against 0 instead of `-blur` would classify a
		// tile sitting in the outer half of the ramp as `FullyOutside`, and the
		// operation drops such tiles — punching tile-shaped holes into the gradient.
		let corners = [[tx_min, ty_min], [tx_min, ty_max], [tx_max, ty_min], [tx_max, ty_max]];

		let mut all_inside = true;
		let mut all_outside = true;

		for corner in &corners {
			let signed_dist = self.signed_distance(*corner);

			if signed_dist >= self.blur_meters {
				// Corner is fully opaque (past the inner end of the ramp)
				all_outside = false;
			} else if signed_dist <= -self.blur_meters {
				// Corner is fully transparent (past the outer end of the ramp)
				all_inside = false;
			} else {
				// Corner is somewhere in the ramp
				all_inside = false;
				all_outside = false;
			}
		}

		// Also check center point for better classification
		let center = [f64::midpoint(tx_min, tx_max), f64::midpoint(ty_min, ty_max)];
		let center_dist = self.signed_distance(center);

		if center_dist >= self.blur_meters {
			all_outside = false;
		} else if center_dist <= -self.blur_meters {
			all_inside = false;
		} else {
			all_inside = false;
			all_outside = false;
		}

		// Corners and centre are five samples of a continuous field, so agreeing among
		// themselves is not enough: the boundary can pass between them. Only a centre
		// further from the boundary than the tile's own half-diagonal — plus the half
		// of the ramp that lies on the tile's side of it — rules that out.
		let tile_half_diagonal = ((tx_max - tx_min).powi(2) + (ty_max - ty_min).powi(2)).sqrt() / 2.0;

		if all_inside && center_dist > tile_half_diagonal + self.blur_meters {
			return TileClassification::FullyInside;
		}

		if all_outside && center_dist < -(tile_half_diagonal + self.blur_meters) {
			return TileClassification::FullyOutside;
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

	/// Convert signed distance to alpha value using the configured blur.
	#[must_use]
	#[cfg(test)]
	pub fn distance_to_alpha(&self, signed_dist: f64) -> u8 {
		self.distance_to_alpha_with_blur(signed_dist, self.blur_meters)
	}

	/// Convert signed distance to alpha value with an explicit blur distance.
	///
	/// Used by `compute_alpha_grid` to apply per-tile antialiasing
	/// (effective blur = max of configured blur and 1 pixel size).
	///
	/// # Arguments
	/// * `signed_dist` - Signed distance in meters
	/// * `blur` - Blur distance in meters
	///
	/// # Returns
	/// Alpha value in range [0, 255]
	#[must_use]
	fn distance_to_alpha_with_blur(&self, signed_dist: f64, blur: f64) -> u8 {
		if blur <= 0.0 {
			return if signed_dist > 0.0 { 255 } else { 0 };
		}

		// Normalize to [0, 1] over blur range
		// At signed_dist = blur: t = 1.0 (fully inside)
		// At signed_dist = 0: t = 0.5 (halfway)
		// At signed_dist = -blur: t = 0.0 (fully outside)
		let t = f64::midpoint(signed_dist / blur, 1.0);
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

	/// Get the area the mask can still tint, as a WGS84 geographic bounding box.
	///
	/// This is the polygon's own bounds grown by [`Self::outer_threshold`], the distance
	/// the buffer and the blur ramp together reach *outside* the geometry. The caller
	/// clips the tile pyramid to it, so anything left out here is never rendered: bounding
	/// the raw polygon would cut the outer half of the blur ramp off along a straight
	/// line, which is exactly the artefact the ramp exists to avoid.
	///
	/// Converts the resulting Mercator bounds back to longitude/latitude degrees.
	#[must_use]
	pub fn geo_bbox(&self) -> GeoBBox {
		// A negative buffer larger than the blur pulls the tinted area inside the
		// polygon's bounds, but shrinking the box risks clipping a coastline that the
		// bounds only just contain, so only growth is applied.
		let reach = self.outer_threshold.max(0.0);
		let [mx_min, my_min, mx_max, my_max] = self.bounds_mercator;
		let (mx_min, my_min) = (mx_min - reach, my_min - reach);
		let (mx_max, my_max) = (mx_max + reach, my_max + reach);

		// Inverse spherical Mercator: x (meters) → longitude (degrees)
		let lon_min = (mx_min / EARTH_RADIUS).to_degrees();
		let lon_max = (mx_max / EARTH_RADIUS).to_degrees();

		// Inverse spherical Mercator: y (meters) → latitude (degrees)
		// lat = 2 * atan(exp(y / R)) - PI/2
		let lat_min = (2.0 * (my_min / EARTH_RADIUS).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
		let lat_max = (2.0 * (my_max / EARTH_RADIUS).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();

		// Growing by the reach can push the box past the antimeridian or the poles;
		// clamping keeps it a valid geographic box instead of failing the check and
		// silently skipping the clip.
		GeoBBox::new_normalized(lon_min, lat_min, lon_max, lat_max)
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

		for edge in self.edge_rtree.locate_in_envelope_intersecting(envelope) {
			// Only count edges that actually span y (the envelope query might include
			// edges that just touch y at their endpoints)
			if edge.ray_crosses(x, y) {
				crossings += 1;
			}
		}

		// Odd number of crossings means inside
		crossings % 2 == 1
	}

	/// Compute alpha values for a tile.
	///
	/// The work is split so that neither half is quadratic:
	///
	/// * **Inside/outside** comes from a single scanline pass. Parity is seeded at
	///   the tile's right border — [`EdgeSegment::ray_crosses`] casts toward +∞ —
	///   and then walked leftwards across that row's own crossings, so a pixel
	///   costs a binary search instead of a fresh ray cast against every edge.
	/// * **Distance** is computed only where it can still change the answer. The
	///   alpha ramp has saturated once a pixel is further than `|buffer| + blur`
	///   from the boundary, so each edge stamps its own neighbourhood and every
	///   pixel that no edge reached is decided by its sign alone.
	///
	/// Together that makes the cost proportional to *edges + covered pixels*
	/// rather than *pixels × edges*. The previous version sampled five points per
	/// 32×32 block to skip whole blocks, which both missed slivers that crossed a
	/// block without touching a sample point and still left every surviving pixel
	/// paying a full [`Self::distance_to_nearest_edge`] scan.
	///
	/// Applies automatic antialiasing: the effective blur is at least 1 pixel wide,
	/// producing smooth edges even when no explicit blur is configured.
	///
	/// # Arguments
	/// * `tile_bbox` - Tile bounding box in Mercator meters [x_min, y_min, x_max, y_max]
	/// * `width` - Tile width in pixels
	/// * `height` - Tile height in pixels
	///
	/// # Returns
	/// A vector of alpha values (0-255) for each pixel, in row-major order.
	#[must_use]
	#[allow(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		clippy::cast_precision_loss
	)]
	pub fn compute_alpha_grid(&self, tile_bbox: [f64; 4], width: u32, height: u32) -> Vec<u8> {
		let [x_min, y_min, x_max, y_max] = tile_bbox;
		let (w, h) = (width as usize, height as usize);
		let px_width = (x_max - x_min) / f64::from(width);
		let px_height = (y_max - y_min) / f64::from(height);

		// Use at least 1 pixel of blur for antialiasing
		let pixel_size = px_width.max(px_height);
		let effective_blur = self.blur_meters.max(pixel_size);

		// Past this distance from the boundary the ramp has saturated and only the
		// sign still matters. `buffer` slides the ramp away from the boundary in
		// either direction, so it widens the band that needs a real distance.
		let band = self.buffer_meters.abs() + effective_blur;

		// One R-tree query feeds both halves. It reaches `band` beyond the tile on
		// three sides — enough for any edge that can be a pixel's nearest — and all
		// the way to the mask's right edge, because seeding parity on the right has
		// to see every edge out there. Nothing further left is needed: those edges
		// lie left of every pixel in the tile and so never flip one's parity.
		let [_, _, mask_x_max, _] = self.bounds_mercator;
		let envelope = AABB::from_corners(
			[x_min - band, y_min - band],
			[mask_x_max.max(x_max + band), y_max + band],
		);

		// Pixel centres, and the rows/columns a Mercator span covers. Both clamp
		// before casting so a far-away edge cannot produce an out-of-range index.
		let center_x = |px: usize| x_min + (px as f64 + 0.5) * px_width;
		let center_y = |py: usize| y_max - (py as f64 + 0.5) * px_height;
		let rows_between = |lo: f64, hi: f64| -> Range<usize> {
			// Rows run top to bottom, so a larger y means a smaller index.
			let first = ((y_max - hi) / px_height - 0.5).ceil().clamp(0.0, h as f64);
			let last = ((y_max - lo) / px_height - 0.5).floor().clamp(-1.0, h as f64 - 1.0);
			if last < 0.0 {
				return 0..0;
			}
			first as usize..last as usize + 1
		};
		let cols_between = |lo: f64, hi: f64| -> Range<usize> {
			let first = ((lo - x_min) / px_width - 0.5).ceil().clamp(0.0, w as f64);
			let last = ((hi - x_min) / px_width - 0.5).floor().clamp(-1.0, w as f64 - 1.0);
			if last < 0.0 {
				return 0..0;
			}
			first as usize..last as usize + 1
		};

		// Where each row's edges cross it, how many crossings sit right of the
		// tile, and the squared distance to the nearest edge that reached a pixel.
		let mut crossings: Vec<Vec<f64>> = vec![Vec::new(); h];
		let mut seed_parity = vec![0u32; h];
		let mut dist_sq = vec![f64::INFINITY; w * h];

		for edge in self.edge_rtree.locate_in_envelope_intersecting(envelope) {
			let (ex_lo, ex_hi) = (edge.start[0].min(edge.end[0]), edge.start[0].max(edge.end[0]));
			let (ey_lo, ey_hi) = (edge.start[1].min(edge.end[1]), edge.start[1].max(edge.end[1]));

			// Sign: note where this edge crosses each row it spans. Crossings right
			// of the tile go into the seed; the rest are walked through below.
			for py in rows_between(ey_lo, ey_hi) {
				if let Some(cx) = edge.crossing_x(center_y(py)) {
					if cx > x_max {
						seed_parity[py] += 1;
					} else {
						crossings[py].push(cx);
					}
				}
			}

			// Distance: stamp the pixels this edge could still be nearest to. A
			// point within `band` of the segment is within `band` of the segment's
			// envelope too, so widening the envelope misses nothing.
			let cols = cols_between(ex_lo - band, ex_hi + band);
			for py in rows_between(ey_lo - band, ey_hi + band) {
				let cy = center_y(py);
				for px in cols.clone() {
					let d = edge.distance_squared_to_point([center_x(px), cy]);
					let slot = &mut dist_sq[py * w + px];
					if d < *slot {
						*slot = d;
					}
				}
			}
		}

		let mut alpha = vec![0u8; w * h];
		for (py, row) in crossings.iter_mut().enumerate() {
			row.sort_unstable_by(f64::total_cmp);
			let seeded_inside = seed_parity[py] % 2 == 1;

			for px in 0..w {
				let i = py * w + px;
				// Parity at the right border, flipped once per crossing passed
				// while walking left to this pixel.
				let to_the_right = row.len() - row.partition_point(|&c| c <= center_x(px));
				let inside = seeded_inside ^ (to_the_right % 2 == 1);

				let d = dist_sq[i];
				if d.is_infinite() {
					// Further than `band` from every edge, so the ramp is saturated
					// and the sign alone gives the same answer the formula would.
					alpha[i] = u8::from(inside) * 255;
					continue;
				}

				let raw = d.sqrt();
				let raw_signed = if inside { raw } else { -raw };
				alpha[i] = self.distance_to_alpha_with_blur(raw_signed + self.buffer_meters, effective_blur);
			}
		}

		alpha
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

		for edge in self.edge_rtree.locate_in_envelope_intersecting(envelope) {
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
fn extract_edges(multi_poly: &MultiPolygon<f64>) -> Vec<EdgeSegment> {
	let mut edges = Vec::new();

	for poly in &multi_poly.0 {
		extract_ring_edges(poly.exterior(), &mut edges);
		for interior in poly.interiors() {
			extract_ring_edges(interior, &mut edges);
		}
	}

	edges
}

/// Extract edges from a ring and add them to the edges vector.
fn extract_ring_edges(ring: &LineString<f64>, edges: &mut Vec<EdgeSegment>) {
	let coords = &ring.0;

	for window in coords.windows(2) {
		if let [c1, c2] = window {
			edges.push(EdgeSegment::new([c1.x, c1.y], [c2.x, c2.y]));
		}
	}
}

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
	use assert_fs::prelude::*;
	use versatiles_core::TileCoord;

	use super::*;

	/// The same reference shape the operation's own tests use: a body with a
	/// notch cut out of the top, an interior ring, and a detached polygon.
	fn oracle_mask(buffer: f64, blur: f64) -> MaskGeometry {
		let file = assert_fs::NamedTempFile::new("oracle.geojson").unwrap();
		file
			.write_str(
				r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
				"geometry":{"type":"MultiPolygon","coordinates":[
				[[[2,2],[18,2],[18,18],[12,18],[12,9],[8,9],[8,18],[2,18],[2,2]],
				 [[5,5],[9,5],[9,7],[5,7],[5,5]]],
				[[[20,12],[24,12],[24,16],[20,16],[20,12]]]]}}]}"#,
			)
			.unwrap();
		MaskGeometry::from_geojson(file.path(), buffer, blur, BlurFunction::default()).unwrap()
	}

	/// The definition of the answer, with no shortcuts at all: every edge is
	/// measured against every point. Deliberately not built on `signed_distance`,
	/// whose R-tree radius makes it exact only near the boundary.
	fn brute_force_alpha(mask: &MaskGeometry, point: [f64; 2], blur: f64) -> u8 {
		let mut min_sq = f64::INFINITY;
		for edge in &mask.edge_rtree {
			min_sq = min_sq.min(edge.distance_squared_to_point(point));
		}
		let dist = min_sq.sqrt();
		let raw_signed = if mask.contains_point_rtree(point[0], point[1]) {
			dist
		} else {
			-dist
		};
		mask.distance_to_alpha_with_blur(raw_signed + mask.buffer_meters, blur)
	}

	/// `compute_alpha_grid` gets its answer from scanline parity plus distances
	/// stamped only near an edge. Both are shortcuts, and both are supposed to be
	/// free rather than approximate: every pixel must equal the brute-force value.
	///
	/// The tiles are chosen to reach the cases a single tile cannot. Parity is
	/// seeded at the tile's right border, so a tile whose right border falls
	/// *inside* the shape is the one that catches a seed left at zero; the
	/// interior ring catches a fill that treats a hole's edges as entries; and
	/// the negative buffer catches a band computed without an absolute value,
	/// which would put the shifted boundary outside the stamped region and drop
	/// the buffer silently.
	#[test]
	fn the_alpha_grid_matches_a_brute_force_reference() {
		for (buffer, blur) in [(0.0, 0.0), (0.0, 60_000.0), (80_000.0, 0.0), (-80_000.0, 20_000.0)] {
			let mask = oracle_mask(buffer, blur);

			for (z, lon, lat) in [
				(4, 10.0, 10.0), // the whole shape inside one tile
				(6, 14.0, 12.0), // right border inside the body: the seed is 1
				(6, 4.0, 10.0),  // left border outside, the body to the right
				(7, 7.0, 6.0),   // straddling the interior ring
				(6, 22.0, 14.0), // the detached polygon
				(6, 30.0, 30.0), // clear of everything
			] {
				let coord = TileCoord::from_geo(lon, lat, z).unwrap();
				let bbox = coord.to_mercator_bbox();
				let [x_min, _, x_max, y_max] = bbox;

				// Both a coarse grid, where a pixel dwarfs the blur and nearly
				// every pixel lands in the stamped band, and a full-size one,
				// where most pixels are past it and decided by sign alone.
				for size in [64u32, 256] {
					let px_width = (x_max - x_min) / f64::from(size);
					let px_height = (y_max - bbox[1]) / f64::from(size);
					let effective_blur = mask.blur_meters.max(px_width.max(px_height));

					let actual = mask.compute_alpha_grid(bbox, size, size);

					for py in 0..size as usize {
						for px in 0..size as usize {
							let point = [
								x_min + (px as f64 + 0.5) * px_width,
								y_max - (py as f64 + 0.5) * px_height,
							];
							let expected = brute_force_alpha(&mask, point, effective_blur);
							assert_eq!(
								actual[py * size as usize + px],
								expected,
								"buffer={buffer} blur={blur} z{z} ({lon},{lat}) {size}px at ({px},{py})"
							);
						}
					}
				}
			}
		}
	}

	/// A mask in the wrong CRS is refused where the mistake is.
	///
	/// The mask is projected on load, and projection clamps: metre-scale
	/// coordinates used to collapse to a degenerate box at the corner of the world
	/// and fail four layers on, in the pyramid clip, with "x (180.0000000001) must
	/// be <= 180" — a message that says nothing about the actual mistake.
	#[test]
	fn a_mask_in_the_wrong_crs_is_refused_by_name() {
		let file = assert_fs::NamedTempFile::new("utm.geojson").unwrap();
		file
			.write_str(
				r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
				"geometry":{"type":"Polygon","coordinates":[[[389298,5819938],[565932,5819938],
				[565932,5933496],[389298,5933496],[389298,5819938]]]}}]}"#,
			)
			.unwrap();

		let err = MaskGeometry::from_geojson(file.path(), 0.0, 0.0, BlurFunction::default()).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("389298"), "{msg}");
		assert!(msg.contains("EPSG:4326"), "{msg}");
		assert!(!msg.contains("TileBBox"), "should fail before the pyramid clip: {msg}");
	}

	/// A mask declaring a non-WGS84 CRS is refused even when its coordinates are
	/// in range — the file says what it is.
	#[test]
	fn a_mask_declaring_another_crs_is_refused() {
		let file = assert_fs::NamedTempFile::new("declared.geojson").unwrap();
		file
			.write_str(
				r#"{"type":"FeatureCollection",
				"crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::25832"}},
				"features":[{"type":"Feature","properties":{},
				"geometry":{"type":"Polygon","coordinates":[[[2,2],[4,2],[4,4],[2,4],[2,2]]]}}]}"#,
			)
			.unwrap();

		let msg = format!(
			"{:#}",
			MaskGeometry::from_geojson(file.path(), 0.0, 0.0, BlurFunction::default()).unwrap_err()
		);
		assert!(msg.contains("EPSG::25832"), "{msg}");
	}

	/// An axis-swapped mask still loads, because nothing can tell it apart.
	///
	/// Pinned so the range check is not later mistaken for protection against it:
	/// both numbers are in range, and the mask lands in the wrong place quietly.
	#[test]
	fn an_axis_swapped_mask_is_not_detectable() {
		let file = assert_fs::NamedTempFile::new("swapped.geojson").unwrap();
		file
			.write_str(
				r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
				"geometry":{"type":"Polygon","coordinates":[[[52.5,9.9],[53.6,9.9],[53.6,13.5],
				[52.5,13.5],[52.5,9.9]]]}}]}"#,
			)
			.unwrap();

		assert!(MaskGeometry::from_geojson(file.path(), 0.0, 0.0, BlurFunction::default()).is_ok());
	}

	/// A tile lying in the *outer* half of the blur ramp must not be classified
	/// `FullyOutside`.
	///
	/// The ramp is centred on the boundary: alpha only reaches 0 at `-blur`, so a tile
	/// that is entirely outside the polygon can still be visibly tinted. The operation
	/// drops whatever this call reports as `FullyOutside`, so getting it wrong deletes
	/// whole tiles out of the gradient and leaves a staircase of tile-sized holes along
	/// the mask edge — the artefact grows with the blur radius and with the zoom level,
	/// because the test it used to fail compared the centre distance against the tile's
	/// own half-diagonal, which shrinks as tiles do.
	#[test]
	fn a_tile_in_the_outer_blur_band_is_not_dropped() {
		let mask = oracle_mask(0.0, 200_000.0);
		// Roughly 2° east of the shape's eastern edge at lon 18: outside the polygon,
		// well inside a 200 km ramp, and small enough at z7 to clear its own diagonal.
		let coord = TileCoord::from_geo(20.0, 17.0, 7).unwrap();
		let bbox = coord.to_mercator_bbox();

		assert_eq!(mask.classify_tile(bbox), TileClassification::Partial);
		assert!(mask.compute_alpha_grid(bbox, 64, 64).iter().any(|&a| a > 0));
	}

	/// The classification must never disagree with the pixels it stands in for.
	///
	/// `FullyInside` passes the tile through untouched and `FullyOutside` deletes it, so
	/// each is a claim about every pixel of the grid. This sweeps the tiles around the
	/// shape's edge at the zoom and blur where the two shortcuts are tightest, and holds
	/// them to that claim.
	#[test]
	fn classification_agrees_with_the_alpha_grid() {
		for blur in [0.0, 50_000.0, 200_000.0] {
			let mask = oracle_mask(0.0, blur);
			for x in 130..146 {
				for y in 108..124 {
					let coord = TileCoord::new(8, x, y).unwrap();
					let bbox = coord.to_mercator_bbox();
					let class = mask.classify_tile(bbox);
					if class == TileClassification::Partial {
						continue;
					}
					let alpha = mask.compute_alpha_grid(bbox, 32, 32);
					match class {
						TileClassification::FullyInside => {
							assert!(alpha.iter().all(|&a| a == 255), "{coord:?} blur={blur} is not opaque");
						}
						TileClassification::FullyOutside => {
							assert!(alpha.iter().all(|&a| a == 0), "{coord:?} blur={blur} is not empty");
						}
						TileClassification::Partial => unreachable!(),
					}
				}
			}
		}
	}

	/// The clipping box has to include the ground the blur and buffer reach.
	///
	/// It is used to trim the tile pyramid, so a box drawn around the bare polygon stops
	/// the outer half of the ramp from ever being rendered — a straight-edged cut across
	/// the gradient wherever the shape approaches its own bounding box.
	#[test]
	fn the_geo_bbox_covers_the_blur_and_buffer_reach() {
		let plain = oracle_mask(0.0, 0.0).geo_bbox();
		let blurred = oracle_mask(0.0, 200_000.0).geo_bbox();
		let buffered = oracle_mask(200_000.0, 0.0).geo_bbox();

		// 200 km is a bit under 2° of longitude at this latitude.
		for grown in [blurred, buffered] {
			assert!(grown.x_min < plain.x_min - 1.5, "{grown:?} vs {plain:?}");
			assert!(grown.y_min < plain.y_min - 1.5, "{grown:?} vs {plain:?}");
			assert!(grown.x_max > plain.x_max + 1.5, "{grown:?} vs {plain:?}");
			assert!(grown.y_max > plain.y_max + 1.5, "{grown:?} vs {plain:?}");
		}
	}

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
			polygon_mercator: MultiPolygon(vec![]),
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
			polygon_mercator: MultiPolygon(vec![]),
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
		// Create a simple square polygon: (0,0) to (100,100)
		let ring = LineString::from(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0], [0.0, 0.0]]);
		let poly = Polygon::new(ring, vec![]);
		let multi = MultiPolygon(vec![poly]);

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
		// Create a large polygon that fully contains the test tile
		let ring = LineString::from(vec![
			[-1000.0, -1000.0],
			[1000.0, -1000.0],
			[1000.0, 1000.0],
			[-1000.0, 1000.0],
			[-1000.0, -1000.0],
		]);
		let poly = Polygon::new(ring, vec![]);
		let multi = MultiPolygon(vec![poly]);

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
		// Create a polygon far from the test tile
		let ring = LineString::from(vec![
			[10000.0, 10000.0],
			[11000.0, 10000.0],
			[11000.0, 11000.0],
			[10000.0, 11000.0],
			[10000.0, 10000.0],
		]);
		let poly = Polygon::new(ring, vec![]);
		let multi = MultiPolygon(vec![poly]);

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
