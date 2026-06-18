//! Per-layer byte breakdown of a decoded vector tile.
//!
//! Shared between the `analyze-tile` dev tool (single-tile drill-down) and
//! `probe -ddd` (whole-container aggregation by zoom × layer). Both need the
//! same notion of "how many bytes does each layer cost, split by geometry, tag
//! references, property table, and feature ids", so it lives here once.
//!
//! All figures are **uncompressed** MVT content — what the user can actually
//! shrink. `encoded_bytes` is the exact serialized layer size; the named
//! categories plus [`LayerStats::other_bytes`] (framing/geom-type/name residual)
//! sum to it.

use anyhow::Result;
use versatiles_geometry::vector_tile::{GeoValuePBF, VectorTile};

/// Accumulated byte breakdown for one layer. Used both per-tile (one decoded
/// layer) and aggregated (summed across many tiles via [`LayerStats::add`]).
#[derive(Clone, Default)]
pub struct LayerStats {
	pub name: String,
	pub feature_count: usize,
	pub vertex_count: usize,
	/// Sum of `feature.geom_data.len()` — the geometry command streams.
	pub geometry_bytes: usize,
	/// Sum of the packed `tag_ids` varint lengths — per-feature property refs.
	pub tag_bytes: usize,
	/// Sum of key-string lengths in the property table.
	pub key_bytes: usize,
	/// Sum of encoded value-message lengths in the property table.
	pub value_bytes: usize,
	/// Sum of encoded feature-id bytes (key byte + id varint) over features that
	/// carry an id. Zero for sources without ids.
	pub id_bytes: usize,
	/// Exact encoded layer size (`layer.to_blob().len()`), used to derive the
	/// residual "other" framing/overhead so the columns sum to the total.
	pub encoded_bytes: usize,
}

impl LayerStats {
	/// Property-table bytes: keys + values (not the per-feature tag refs).
	#[must_use]
	pub fn property_bytes(&self) -> usize {
		self.key_bytes + self.value_bytes
	}

	/// Residual bytes not attributed to geometry/tags/properties/ids: geom-type
	/// fields, the layer name, and all the PBF key/length framing.
	#[must_use]
	pub fn other_bytes(&self) -> usize {
		self
			.encoded_bytes
			.saturating_sub(self.geometry_bytes + self.tag_bytes + self.property_bytes() + self.id_bytes)
	}

	/// Add another layer's byte counts into this one (for cross-tile
	/// aggregation). Leaves `name` untouched.
	pub fn add(&mut self, other: &LayerStats) {
		self.feature_count += other.feature_count;
		self.vertex_count += other.vertex_count;
		self.geometry_bytes += other.geometry_bytes;
		self.tag_bytes += other.tag_bytes;
		self.key_bytes += other.key_bytes;
		self.value_bytes += other.value_bytes;
		self.id_bytes += other.id_bytes;
		self.encoded_bytes += other.encoded_bytes;
	}
}

/// Byte length of `value` as an unsigned LEB128 varint (matches the MVT/PBF
/// packed `tag_ids` and feature-id encoding).
#[must_use]
pub fn varint_len(mut value: u64) -> usize {
	let mut len = 1;
	while value >= 0x80 {
		value >>= 7;
		len += 1;
	}
	len
}

/// Compute the per-layer byte breakdown for every layer in `vt`.
pub fn layer_stats(vt: &VectorTile) -> Result<Vec<LayerStats>> {
	let mut out = Vec::with_capacity(vt.layers.len());
	for layer in &vt.layers {
		let mut stats = LayerStats {
			name: layer.name.clone(),
			feature_count: layer.features.len(),
			encoded_bytes: usize::try_from(layer.to_blob()?.len())?,
			..LayerStats::default()
		};

		for feature in &layer.features {
			stats.geometry_bytes += usize::try_from(feature.geom_data.len())?;
			stats.vertex_count += feature.count_geometry_points();
			stats.tag_bytes += feature
				.tag_ids
				.iter()
				.map(|id| varint_len(u64::from(*id)))
				.sum::<usize>();
			// Feature id (MVT field 1, varint): one key byte + the id varint.
			if let Some(id) = feature.id {
				stats.id_bytes += 1 + varint_len(id);
			}
		}

		for key in layer.property_manager.iter_key() {
			stats.key_bytes += key.len();
		}
		for value in layer.property_manager.iter_val() {
			stats.value_bytes += GeoValuePBF::to_blob(value).map_or(0, |b| usize::try_from(b.len()).unwrap_or(0));
		}

		out.push(stats);
	}
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_geometry::{
		geo::{GeoFeature, GeoProperties, GeoValue},
		vector_tile::{VectorTile, VectorTileLayer},
	};

	use geo_types::{Geometry, LineString, Point};

	#[test]
	fn varint_len_matches_leb128_boundaries() {
		assert_eq!(varint_len(0), 1);
		assert_eq!(varint_len(127), 1);
		assert_eq!(varint_len(128), 2);
		assert_eq!(varint_len(16_383), 2);
		assert_eq!(varint_len(16_384), 3);
	}

	#[test]
	fn layer_stats_splits_geometry_ids_and_properties() -> Result<()> {
		let line = GeoFeature {
			id: Some(GeoValue::from(1_u64)),
			geometry: Geometry::LineString(LineString::from(
				(0..10).map(|i| [f64::from(i), f64::from(i)]).collect::<Vec<_>>(),
			)),
			properties: GeoProperties::from(vec![("name", GeoValue::from("Main Street"))]),
		};
		let point = GeoFeature {
			id: Some(GeoValue::from(2_u64)),
			geometry: Geometry::Point(Point::new(3.0, 4.0)),
			properties: GeoProperties::from(vec![("name", GeoValue::from("POI"))]),
		};
		let layer = VectorTileLayer::from_features("roads".to_string(), vec![line, point], 4096, 1)?;
		let vt = VectorTile::new(vec![layer]);

		let stats = layer_stats(&vt)?;
		assert_eq!(stats.len(), 1);
		let s = &stats[0];
		assert_eq!(s.name, "roads");
		assert_eq!(s.feature_count, 2);
		assert!(s.vertex_count >= 11, "10-point line + 1 point");
		assert!(s.geometry_bytes > 0);
		assert!(s.tag_bytes > 0);
		assert!(s.property_bytes() > 0);
		assert!(s.id_bytes > 0, "both features carry ids");
		// Categories must never overcount the real encoded size.
		assert!(s.geometry_bytes + s.tag_bytes + s.property_bytes() + s.id_bytes <= s.encoded_bytes);
		Ok(())
	}

	#[test]
	fn add_sums_all_byte_categories() {
		let a = LayerStats {
			name: "x".into(),
			feature_count: 1,
			vertex_count: 2,
			geometry_bytes: 3,
			tag_bytes: 4,
			key_bytes: 5,
			value_bytes: 6,
			id_bytes: 7,
			encoded_bytes: 30,
		};
		let mut acc = LayerStats {
			name: "x".into(),
			..LayerStats::default()
		};
		acc.add(&a);
		acc.add(&a);
		assert_eq!(acc.feature_count, 2);
		assert_eq!(acc.geometry_bytes, 6);
		assert_eq!(acc.id_bytes, 14);
		assert_eq!(acc.encoded_bytes, 60);
		// other = encoded - (geom+tag+props+id) = 60 - (6+8+22+14) = 10
		assert_eq!(acc.other_bytes(), 10);
	}
}
