//! Per-layer byte breakdown of a vector tile.
//!
//! "Which layer is eating my tile, and on what" splits into geometry, per-feature
//! tag references, the property table and feature ids — and it is the
//! geometry-versus-properties split that says what to do about it. Re-encoding
//! each layer to measure it yields a total and none of the split, so this exposes
//! [`versatiles_geometry::vector_tile::layer_stats`] rather than approximating it
//! a second time on the JavaScript side.
//!
//! Standalone rather than a method on `TileSource`, so it also works on a tile the
//! caller already holds — from a fetch, a file, or a pipeline — and so
//! `getTile()` + `layerStats()` composes without a second way to do the same thing.

use anyhow::{Result, anyhow, bail};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use versatiles_core::Blob;
use versatiles_geometry::vector_tile::{VectorTile, layer_stats};

use crate::macros::NapiResultExt;

/// Byte breakdown of a single layer within a vector tile.
///
/// All figures are uncompressed MVT content — what can actually be shrunk.
/// The named categories plus `otherBytes` sum exactly to `encodedBytes`, so a
/// stacked bar built from them adds up without a fudge factor.
#[napi(object)]
#[derive(Clone, Debug)]
pub struct LayerStats {
	/// Layer name.
	pub name: String,
	/// Number of features in the layer.
	pub feature_count: u32,
	/// Total number of geometry vertices across all features.
	pub vertex_count: u32,
	/// Geometry command streams.
	pub geometry_bytes: u32,
	/// Per-feature property references (the packed `tag_ids` varints).
	pub tag_bytes: u32,
	/// Key strings in the property table.
	pub key_bytes: u32,
	/// Encoded value messages in the property table.
	pub value_bytes: u32,
	/// `keyBytes + valueBytes` — the whole property table.
	pub property_bytes: u32,
	/// Feature ids. Zero for sources that carry none.
	pub id_bytes: u32,
	/// Framing, geometry-type fields and the layer name: whatever the named
	/// categories do not account for.
	pub other_bytes: u32,
	/// Exact serialized size of the layer.
	pub encoded_bytes: u32,
}

/// Compute the per-layer byte breakdown of an uncompressed vector tile.
///
/// # Arguments
///
/// * `tile` - Uncompressed MVT bytes, as returned by `TileSource.getTile()`.
///
/// # Returns
///
/// One entry per layer, in the order the layers appear in the tile. A tile with
/// no layers — including a zero-byte buffer, which is what an empty tile encodes
/// to — yields an empty array. `getTile()` signals a missing tile with `null`,
/// so an empty buffer means an empty tile rather than a mistake.
///
/// # Errors
///
/// Returns an error if the buffer is not an uncompressed vector tile. Compressed
/// input and raster input are named as such rather than surfacing as a protobuf
/// parse failure.
///
/// # Examples
///
/// ```javascript
/// const { TileSource, layerStats } = require('@versatiles/versatiles-rs');
///
/// const source = await TileSource.fromPath('berlin.mbtiles');
/// const tile = await source.getTile(14, 8802, 5373);
/// if (tile) {
///   for (const layer of layerStats(tile)) {
///     console.log(layer.name, layer.geometryBytes, layer.propertyBytes, layer.encodedBytes);
///   }
/// }
/// ```
#[napi(js_name = "layerStats")]
// napi requires an owned `Buffer` for arguments — `&Buffer` does not implement
// `FromNapiRef`, so the by-value binding is the only shape that compiles.
#[allow(clippy::needless_pass_by_value)]
pub fn layer_stats_of(tile: Buffer) -> napi::Result<Vec<LayerStats>> {
	stats_from_bytes(tile.as_ref()).to_napi()
}

/// The whole of `layerStats` minus the napi wrapper, so it is testable from Rust.
///
/// Named `_of` on the Rust side only because `layer_stats` is the imported
/// upstream function; `js_name` keeps the JavaScript name plain.
fn stats_from_bytes(bytes: &[u8]) -> Result<Vec<LayerStats>> {
	reject_non_mvt(bytes)?;

	let tile =
		VectorTile::from_blob(&Blob::from(bytes)).map_err(|e| anyhow!("not an uncompressed vector tile: {e:#}"))?;

	layer_stats(&tile)?.iter().map(LayerStats::try_from_rust).collect()
}

/// Names the two ways a caller reaches this function with the wrong bytes:
/// a tile that is still compressed, and a raster tile.
///
/// A protobuf parse error for either is technically true and useless — neither
/// says which mistake was made. Brotli has no magic number, so a brotli-compressed
/// tile still falls through to the parse error; the message says so.
fn reject_non_mvt(bytes: &[u8]) -> Result<()> {
	let compression = match bytes {
		[0x1f, 0x8b, ..] => Some("gzip"),
		[0x28, 0xb5, 0x2f, 0xfd, ..] => Some("zstd"),
		_ => None,
	};
	if let Some(compression) = compression {
		bail!(
			"buffer is {compression}-compressed, not an uncompressed vector tile — \
			 TileSource.getTile() returns decompressed bytes, so pass those directly"
		);
	}

	let raster = match bytes {
		[0x89, b'P', b'N', b'G', ..] => Some("PNG"),
		[0xff, 0xd8, 0xff, ..] => Some("JPEG"),
		[b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("WebP"),
		_ => None,
	};
	if let Some(raster) = raster {
		bail!("buffer is a {raster} raster tile, which has no layers to break down");
	}

	Ok(())
}

impl LayerStats {
	/// `property_bytes` and `other_bytes` are methods on the Rust type; a napi
	/// object is plain data, so they are materialised here.
	fn try_from_rust(stats: &versatiles_geometry::vector_tile::LayerStats) -> Result<Self> {
		// A single MVT layer above 4 GiB is not a tile anyone can serve. Erroring
		// beats truncating to u32::MAX, which would report a plausible number.
		fn fit(value: usize, field: &str) -> Result<u32> {
			u32::try_from(value).map_err(|_| anyhow!("layer field '{field}' does not fit in a JS-safe integer"))
		}

		Ok(LayerStats {
			name: stats.name.clone(),
			feature_count: fit(stats.feature_count, "featureCount")?,
			vertex_count: fit(stats.vertex_count, "vertexCount")?,
			geometry_bytes: fit(stats.geometry_bytes, "geometryBytes")?,
			tag_bytes: fit(stats.tag_bytes, "tagBytes")?,
			key_bytes: fit(stats.key_bytes, "keyBytes")?,
			value_bytes: fit(stats.value_bytes, "valueBytes")?,
			property_bytes: fit(stats.property_bytes(), "propertyBytes")?,
			id_bytes: fit(stats.id_bytes, "idBytes")?,
			other_bytes: fit(stats.other_bytes(), "otherBytes")?,
			encoded_bytes: fit(stats.encoded_bytes, "encodedBytes")?,
		})
	}
}

#[cfg(test)]
mod tests {
	use geo_types::{Geometry, Point};
	use versatiles_core::TileCompression;
	use versatiles_geometry::{
		geo::{GeoFeature, GeoProperties, GeoValue},
		vector_tile::VectorTileLayer,
	};

	use super::*;

	/// A tile with one layer holding a point feature that carries an id and a property,
	/// so every byte category is non-zero.
	fn sample_tile() -> Blob {
		let feature = GeoFeature {
			id: Some(GeoValue::from(1_u64)),
			geometry: Geometry::Point(Point::new(3.0, 4.0)),
			properties: GeoProperties::from(vec![("name", GeoValue::from("Berlin"))]),
		};
		let layer = VectorTileLayer::from_features("places".to_string(), vec![feature], 4096, 1).unwrap();
		VectorTile::new(vec![layer]).to_blob().unwrap()
	}

	#[test]
	fn stats_categories_sum_to_encoded_bytes() {
		let blob = sample_tile();
		let stats = stats_from_bytes(blob.as_slice()).unwrap();
		assert_eq!(stats.len(), 1);

		let layer = &stats[0];
		assert_eq!(layer.name, "places");
		assert_eq!(layer.feature_count, 1);
		assert!(layer.geometry_bytes > 0);
		assert!(layer.id_bytes > 0);
		// The property the docs promise, so a consumer can draw a stacked bar
		// without wondering whether it adds up.
		assert_eq!(
			layer.geometry_bytes + layer.tag_bytes + layer.property_bytes + layer.id_bytes + layer.other_bytes,
			layer.encoded_bytes
		);
		assert_eq!(layer.property_bytes, layer.key_bytes + layer.value_bytes);
	}

	#[test]
	fn empty_tile_yields_no_layers() {
		// A tile with no layers encodes to zero bytes, so this is also the
		// empty-buffer case: it is a valid empty tile, not bad input.
		let blob = VectorTile::new(vec![]).to_blob().unwrap();
		assert!(blob.is_empty());
		assert!(stats_from_bytes(blob.as_slice()).unwrap().is_empty());
	}

	#[test]
	fn gzipped_input_says_so() {
		let gzipped = versatiles_core::compression::compress(sample_tile(), &TileCompression::Gzip).unwrap();
		let err = stats_from_bytes(gzipped.as_slice()).unwrap_err().to_string();
		assert!(err.contains("gzip-compressed"), "unexpected message: {err}");
		assert!(err.contains("getTile()"), "message should point at the fix: {err}");
	}

	#[test]
	fn raster_input_says_so() {
		let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
		let err = stats_from_bytes(&png).unwrap_err().to_string();
		assert!(err.contains("PNG"), "unexpected message: {err}");
	}

	#[test]
	fn empty_buffer_is_an_empty_tile_not_an_error() {
		assert!(stats_from_bytes(&[]).unwrap().is_empty());
	}

	#[test]
	fn garbage_input_is_an_error_not_a_panic() {
		let err = stats_from_bytes(&[0x42; 32]).unwrap_err().to_string();
		assert!(err.contains("not an uncompressed vector tile"), "unexpected: {err}");
	}
}
