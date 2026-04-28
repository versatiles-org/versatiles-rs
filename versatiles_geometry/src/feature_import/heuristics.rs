//! Heuristics for picking pipeline parameters from input data.
//!
//! Currently just one: [`auto_max_zoom`], which inspects the median feature
//! size and picks the zoom level where it renders at ≈ 4 tile-pixels.

use super::TILE_EXTENT;
use crate::ext::MercatorExt;
use crate::geo::GeoFeature;
use geo::{Area, Euclidean, Length};
use geo_types::Geometry;
use versatiles_core::WORLD_SIZE;

/// Pick the `max_zoom` where the median feature size renders at ≈ 4 tile-pixels.
///
/// Input features must be in **WGS84 lon/lat**; they're projected internally.
/// For point-only inputs (no length/area to measure) the result is the
/// hard cap `MAX_ZOOM = 14` — point density isn't taken into account in v1
/// (known limitation; future versions may use median nearest-neighbor distance).
///
/// Most callers should set [`crate::feature_import::FeatureImportConfig::max_zoom`]
/// to `None` instead of calling this directly — that path projects each
/// geometry once instead of twice (this function clones every input geometry
/// to project it).
#[must_use]
pub fn auto_max_zoom(features_wgs84: &[GeoFeature]) -> u8 {
	let projected: Vec<GeoFeature> = features_wgs84
		.iter()
		.map(|f| GeoFeature {
			id: f.id.clone(),
			geometry: f.geometry.clone().to_mercator(),
			properties: f.properties.clone(),
		})
		.collect();
	auto_max_zoom_projected(&projected)
}

/// Internal: same as [`auto_max_zoom`] but expects features already in mercator.
/// Avoids an extra projection pass when the caller has them projected.
pub(super) fn auto_max_zoom_projected(features_mercator: &[GeoFeature]) -> u8 {
	const MAX_ZOOM: u8 = 14;
	const TARGET_PX: f64 = 4.0;

	let mut sizes: Vec<f64> = features_mercator
		.iter()
		.filter_map(|f| feature_size_mercator(&f.geometry))
		.filter(|s| s.is_finite() && *s > 0.0)
		.collect();
	if sizes.is_empty() {
		return MAX_ZOOM;
	}
	sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let median = sizes[sizes.len() / 2];

	// At zoom z, a size of `median` mercator-meters renders as
	// `median * 2^z * TILE_EXTENT / WORLD_SIZE` pixels. Solve for the z where
	// that equals TARGET_PX:
	//     2^z = TARGET_PX * WORLD_SIZE / (median * TILE_EXTENT)
	//     z = log2(...)
	let zoom_f = (TARGET_PX * WORLD_SIZE / (median * f64::from(TILE_EXTENT))).log2();
	if !zoom_f.is_finite() {
		return MAX_ZOOM;
	}
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	let z = zoom_f.round().clamp(0.0, f64::from(MAX_ZOOM)) as u8;
	z
}

fn feature_size_mercator(g: &Geometry<f64>) -> Option<f64> {
	match g {
		Geometry::LineString(ls) => Some(Euclidean.length(ls)),
		Geometry::MultiLineString(ml) => Some(Euclidean.length(ml)),
		Geometry::Polygon(p) => Some(p.unsigned_area().sqrt()),
		Geometry::MultiPolygon(mp) => Some(mp.unsigned_area().sqrt()),
		_ => None,
	}
}
