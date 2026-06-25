//! MVT tile repairer.
//!
//! [`repair_tile`] takes a decoded [`VectorTile`], runs the validator, and
//! attempts to fix every reported issue in one pass. The function is the
//! canonical entry point for making a tile MVT 2.1 conformant; the pipeline's
//! `vector_repair` VPL operation is a thin wrapper around it.
//!
//! ## Repair strategy
//!
//! ### Always applied
//!
//! - **`MissingExtent`**: sets `layer.extent = Some(4096)`.
//! - **`MissingVersion`**: sets `layer.version = Some(1)`.
//! - **`DuplicateLayerName`**: the first layer with a given name is kept; all
//!   subsequent duplicates are dropped.
//! - **`OrphanInnerRing`** / **`DegenerateRing`**: the layer is rebuilt through
//!   [`VectorTileLayer::from_features`], which normalises polygon ring winding
//!   and drops rings that round to fewer than 3 distinct grid points.
//!
//! ### Controlled by `drop_offenders`
//!
//! Some features cannot be decoded at all (geometry byte stream is corrupt) or
//! carry geometry type 0 with non-empty data (ambiguous). When `drop_offenders`
//! is `true` these features are silently removed from the output. When it is
//! `false` (the default), the geometry rebuild for that layer is skipped —
//! structural fixes (extent, version) are still applied, but the features are
//! left as they are in the wire bytes, preserving every byte of the original
//! payload.

use super::{VectorTile, VectorTileLayer, validate_tile};
use anyhow::Result;
use std::collections::HashSet;

/// Repair `tile` so that it passes [`validate_tile`].
///
/// Pass `drop_offenders = true` to have features that cannot be decoded
/// removed from the output. With the default `false`, any layer whose
/// features cannot all be decoded is left structurally fixed (extent /
/// version set) but its geometry is not rebuilt.
///
/// Returns the repaired tile. If the tile is already conformant, the
/// returned value is logically identical to the input (the fast path returns
/// it without cloning).
pub fn repair_tile(tile: VectorTile, drop_offenders: bool) -> Result<VectorTile> {
	let issues = validate_tile(&tile);
	if issues.is_empty() {
		return Ok(tile);
	}

	let mut seen_names: HashSet<String> = HashSet::new();
	let mut new_layers: Vec<VectorTileLayer> = Vec::with_capacity(tile.layers.len());

	for mut layer in tile.layers {
		// DuplicateLayerName: keep only the first occurrence.
		if !seen_names.insert(layer.name.clone()) {
			continue;
		}

		// MissingExtent / MissingVersion: set defaults in-place.
		if layer.extent.is_none() {
			layer.extent = Some(4096);
		}
		if layer.version.is_none() {
			layer.version = Some(1);
		}

		// Check whether this layer has any feature-level geometry issues.
		let has_feature_issues = issues
			.iter()
			.any(|i| i.layer == layer.name && i.feature_index.is_some());

		if has_feature_issues {
			let extent = layer.extent.unwrap_or(4096);
			let version = layer.version.unwrap_or(1);
			let name = layer.name.clone();

			let mut geo_features = Vec::with_capacity(layer.features.len());
			let mut decode_failed = false;

			for feature in &layer.features {
				match feature.to_feature_lenient(&layer) {
					Ok(gf) => geo_features.push(gf),
					Err(_) if drop_offenders => {
						// Drop this feature silently.
					}
					Err(_) => {
						// Cannot decode; leave this layer's geometry untouched.
						decode_failed = true;
						break;
					}
				}
			}

			if !decode_failed {
				// All features decoded (or offenders were dropped): rebuild.
				layer = VectorTileLayer::from_features(name, geo_features, extent, version)?;
			}
			// If decode_failed && !drop_offenders: layer keeps structural fixes
			// but geometry bytes are left as-is (features unchanged).
		}

		new_layers.push(layer);
	}

	Ok(VectorTile::new(new_layers))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vector_tile::{IssueKind, VectorTileLayer, validate_tile};
	use crate::vector_tile::feature::VectorTileFeature;
	use crate::vector_tile::geometry_type::GeomType;
	use versatiles_core::Blob;
	use versatiles_core::io::{ValueWriter, ValueWriterBlob};

	fn raw_polygon_feature(rings: &[Vec<(i32, i32)>]) -> VectorTileFeature {
		let mut writer = ValueWriterBlob::new_le();
		let mut prev = (0i64, 0i64);
		for ring in rings {
			assert!(!ring.is_empty());
			let (fx, fy) = ring[0];
			let (ix, iy) = (i64::from(fx), i64::from(fy));
			writer.write_varint((1 << 3) | 0x1).unwrap();
			writer.write_svarint(ix - prev.0).unwrap();
			writer.write_svarint(iy - prev.1).unwrap();
			prev = (ix, iy);
			let rest = ring.len() - 1;
			if rest > 0 {
				writer.write_varint(((rest as u64) << 3) | 0x2).unwrap();
				for &(fx, fy) in &ring[1..] {
					let (ix, iy) = (i64::from(fx), i64::from(fy));
					writer.write_svarint(ix - prev.0).unwrap();
					writer.write_svarint(iy - prev.1).unwrap();
					prev = (ix, iy);
				}
			}
			writer.write_varint(7).unwrap(); // ClosePath
		}
		VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::MultiPolygon,
			geom_data: writer.into_blob(),
		}
	}

	fn malformed_feature() -> VectorTileFeature {
		VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::MultiPolygon,
			geom_data: Blob::from(vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
		}
	}

	fn layer_with(name: &str, features: Vec<VectorTileFeature>) -> VectorTileLayer {
		let mut layer = VectorTileLayer::new(name.to_string(), 4096, 1);
		layer.features = features;
		layer
	}

	#[test]
	fn clean_tile_is_returned_unchanged() -> Result<()> {
		let outer = vec![(0, 0), (4, 0), (4, 4), (0, 4)];
		let feature = raw_polygon_feature(&[outer]);
		let layer = layer_with("roads", vec![feature]);
		let tile = VectorTile::new(vec![layer]);

		let repaired = repair_tile(tile, false)?;
		let issues = validate_tile(&repaired);
		assert!(issues.is_empty(), "repaired tile should be clean: {issues:?}");
		Ok(())
	}

	#[test]
	fn repairs_missing_extent() -> Result<()> {
		let mut layer = VectorTileLayer::new("l".to_string(), 4096, 1);
		layer.extent = None;
		let tile = VectorTile::new(vec![layer]);

		let repaired = repair_tile(tile, false)?;
		assert_eq!(repaired.layers[0].extent, Some(4096));
		assert!(validate_tile(&repaired).is_empty());
		Ok(())
	}

	#[test]
	fn repairs_missing_version() -> Result<()> {
		let mut layer = VectorTileLayer::new("l".to_string(), 4096, 1);
		layer.version = None;
		let tile = VectorTile::new(vec![layer]);

		let repaired = repair_tile(tile, false)?;
		assert_eq!(repaired.layers[0].version, Some(1));
		assert!(validate_tile(&repaired).is_empty());
		Ok(())
	}

	#[test]
	fn drops_duplicate_layer_names() -> Result<()> {
		let a = VectorTileLayer::new("roads".to_string(), 4096, 1);
		let b = VectorTileLayer::new("roads".to_string(), 4096, 1);
		let tile = VectorTile::new(vec![a, b]);

		let repaired = repair_tile(tile, false)?;
		assert_eq!(repaired.layers.len(), 1);
		assert_eq!(repaired.layers[0].name, "roads");
		assert!(validate_tile(&repaired).is_empty());
		Ok(())
	}

	#[test]
	fn repairs_orphan_inner_ring() -> Result<()> {
		// CCW ring = orphan inner (no preceding outer).
		let orphan = vec![(0, 0), (0, 4), (4, 4), (4, 0)];
		let feature = raw_polygon_feature(&[orphan]);
		let layer = layer_with("l", vec![feature]);
		let tile = VectorTile::new(vec![layer]);

		let pre = validate_tile(&tile);
		assert!(pre.iter().any(|i| i.kind == IssueKind::OrphanInnerRing));

		let repaired = repair_tile(tile, false)?;
		let post = validate_tile(&repaired);
		assert!(
			post.iter().all(|i| i.kind != IssueKind::OrphanInnerRing),
			"orphan inner ring should be gone after repair: {post:?}"
		);
		Ok(())
	}

	#[test]
	fn drop_offenders_true_removes_malformed_feature() -> Result<()> {
		let good = raw_polygon_feature(&[vec![(0, 0), (4, 0), (4, 4), (0, 4)]]);
		let layer = layer_with("l", vec![good, malformed_feature()]);
		let tile = VectorTile::new(vec![layer]);

		let repaired = repair_tile(tile, true)?;
		assert_eq!(repaired.layers[0].features.len(), 1, "malformed feature should be dropped");
		Ok(())
	}

	#[test]
	fn drop_offenders_false_keeps_layer_geometry_on_decode_failure() -> Result<()> {
		// When drop_offenders is false and a feature can't be decoded, the
		// geometry rebuild is skipped — the layer keeps its original features.
		let good = raw_polygon_feature(&[vec![(0, 0), (4, 0), (4, 4), (0, 4)]]);
		let layer = layer_with("l", vec![good, malformed_feature()]);
		let original_feature_count = 2;
		let tile = VectorTile::new(vec![layer]);

		let repaired = repair_tile(tile, false)?;
		assert_eq!(
			repaired.layers[0].features.len(),
			original_feature_count,
			"features should be unchanged when rebuild is skipped"
		);
		Ok(())
	}

	#[test]
	fn structural_fixes_applied_even_when_geometry_rebuild_skipped() -> Result<()> {
		let mut layer = VectorTileLayer::new("l".to_string(), 4096, 1);
		layer.extent = None;
		layer.version = None;
		layer.features = vec![malformed_feature()];
		let tile = VectorTile::new(vec![layer]);

		// drop_offenders: false → geometry not rebuilt, but structural fixes applied
		let repaired = repair_tile(tile, false)?;
		assert_eq!(repaired.layers[0].extent, Some(4096));
		assert_eq!(repaired.layers[0].version, Some(1));
		Ok(())
	}
}
