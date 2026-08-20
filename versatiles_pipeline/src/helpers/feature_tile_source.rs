//! Shared `TileSource` adapter for the read operations that synthesise vector
//! tiles from in-memory feature data (`from_geo`, `from_csv`).
//!
//! Both operations did almost the same thing — load features → build a
//! [`FeatureImport`] → expose it as a `TileSource` of MVT tiles. The
//! per-format work (parsing the input file, dispatching extensions) stays in
//! each op; everything *after* a `FeatureImport` is built lives here.
//!
//! This module exposes:
//!
//! - [`apply_property_filters`] — implements `properties_include=` /
//!   `properties_exclude=` over a `Vec<GeoFeature>`.
//! - [`FeatureTileSource`] — the concrete `TileSource` impl. Each op
//!   constructs one of these from its [`FeatureImport`] + chosen settings
//!   and returns it from the `ReadTileSource::build` factory.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream};
use versatiles_geometry::{feature_import::FeatureImport, geo::GeoFeature};

use crate::helpers::{
	tile_error_monitor::{TileErrorMonitor, TileErrorStage},
	tile_size_monitor::{MaxTileBytes, TileBreakdown, TileSizeMonitor, resolve_hard_cap},
	tilejson::set_single_vector_layer,
};

/// Apply `properties_include` (whitelist) or `properties_exclude` (blacklist)
/// to every feature. Either argument may be `None` — the caller has already
/// rejected the case where both are `Some`.
pub fn apply_property_filters(features: &mut [GeoFeature], include: Option<&[String]>, exclude: Option<&[String]>) {
	use std::collections::HashSet;
	if let Some(keep) = include {
		let keep: HashSet<&str> = keep.iter().map(String::as_str).collect();
		for f in features.iter_mut() {
			f.properties.retain(|k, _| keep.contains(k.as_str()));
		}
	} else if let Some(drop) = exclude {
		let drop: HashSet<&str> = drop.iter().map(String::as_str).collect();
		for f in features.iter_mut() {
			f.properties.retain(|k, _| !drop.contains(k.as_str()));
		}
	}
}

/// Concrete `TileSource` shared by `from_geo` and `from_csv`. Wraps a
/// fully-built [`FeatureImport`] plus per-operation runtime state
/// (compression, size/error monitors, source-type labels).
pub struct FeatureTileSource {
	import: Arc<FeatureImport>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	compression: TileCompression,
	size_monitor: TileSizeMonitor,
	error_monitor: TileErrorMonitor,
	/// `SourceType::new_container` description, e.g. `"geo features"`.
	source_description: &'static str,
	/// `SourceType::new_container` short name, e.g. `"geo"`.
	source_short: &'static str,
}

impl FeatureTileSource {
	/// Build a [`FeatureTileSource`] from a populated [`FeatureImport`].
	///
	/// `label` identifies the operation in monitor log lines (e.g.
	/// `"from_geo"`); `source_description` and `source_short` go into the
	/// `SourceType` used by tooling that introspects the pipeline.
	///
	/// `max_tile_bytes` sets the hard cap on encoded tile size: `None` keeps
	/// the default 1 MiB, [`MaxTileBytes::Disabled`] switches the cap off, and
	/// [`MaxTileBytes::Limit`] caps encoded tiles at that many bytes. Tiles
	/// above the cap are skipped by the streaming path and error out on the
	/// single-tile path; the soft-cap warning threshold scales with the cap.
	pub fn new(
		import: FeatureImport,
		layer_name: &str,
		compression: TileCompression,
		label: &'static str,
		source_description: &'static str,
		source_short: &'static str,
		max_tile_bytes: Option<MaxTileBytes>,
	) -> Result<Self> {
		// Tile pyramid covers the data bbox over [min_zoom, max_zoom];
		// for empty input, an empty pyramid.
		let pyramid = match import.bounds_geo()? {
			Some(bbox) => TilePyramid::from_geo_bbox(import.min_zoom(), import.max_zoom(), &bbox)?,
			None => TilePyramid::new_empty(),
		};
		let metadata = TileSourceMetadata::new(TileFormat::MVT, compression, Traversal::ANY, Some(pyramid));

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", layer_name)?;
		// Vector consumers like QGIS need the TileJSON `vector_layers` entry
		// to know what's in each MVT layer; set one entry covering this
		// layer's fields and zoom range.
		populate_vector_layers(&mut tilejson, layer_name, &import);
		metadata.update_tilejson(&mut tilejson);

		Ok(Self {
			import: Arc::new(import),
			metadata,
			tilejson,
			compression,
			size_monitor: TileSizeMonitor::new(label, resolve_hard_cap(max_tile_bytes)),
			error_monitor: TileErrorMonitor::new(label),
			source_description,
			source_short,
		})
	}
}

impl std::fmt::Debug for FeatureTileSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FeatureTileSource")
			.field("source", &self.source_short)
			.field("metadata", &self.metadata)
			.finish()
	}
}

#[async_trait]
impl TileSource for FeatureTileSource {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container(self.source_description, self.source_short)
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		match self.import.get_tile(coord.level, coord.x, coord.y)? {
			Some(vector_tile) => {
				// Compute the breakdown before `Tile::from_vector` consumes
				// the `VectorTile` — the size monitor uses it in any
				// soft-cap warning or hard-cap error message.
				let breakdown = TileBreakdown::from_vector_tile(&vector_tile);
				let mut tile = Tile::from_vector(vector_tile, TileFormat::MVT)?;
				tile.change_compression(&self.compression)?;
				let blob = tile.as_blob(&self.compression)?;
				self.size_monitor.check(*coord, blob, breakdown)?;
				Ok(Some(tile))
			}
			None => Ok(None),
		}
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		let import = Arc::clone(&self.import);
		let compression = self.compression;
		let size_monitor = self.size_monitor.clone();
		let error_monitor = self.error_monitor.clone();
		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			let vector_tile = match import.get_tile(coord.level, coord.x, coord.y) {
				Ok(Some(vt)) => vt,
				Ok(None) => return None,
				Err(e) => {
					error_monitor.record(coord, TileErrorStage::Render, &e);
					return None;
				}
			};
			// Compute the breakdown before `Tile::from_vector` moves out
			// of `vector_tile`; the monitor needs it for any over-cap
			// warning or summary line.
			let breakdown = TileBreakdown::from_vector_tile(&vector_tile);
			let mut tile = match Tile::from_vector(vector_tile, TileFormat::MVT) {
				Ok(t) => t,
				Err(e) => {
					error_monitor.record(coord, TileErrorStage::Wrap, &e);
					return None;
				}
			};
			if let Err(e) = tile.change_compression(&compression) {
				error_monitor.record(coord, TileErrorStage::Compress, &e);
				return None;
			}
			let blob = match tile.as_blob(&compression) {
				Ok(b) => b,
				Err(e) => {
					error_monitor.record(coord, TileErrorStage::Serialize, &e);
					return None;
				}
			};
			if let Err(e) = size_monitor.check(coord, blob, breakdown) {
				// Hard-cap violation: the size monitor's own one-shot warning
				// will fire from inside `check`; we still record it through
				// the error monitor so the end-of-run summary captures it.
				error_monitor.record(coord, TileErrorStage::OverHardCap, &e);
				return None;
			}
			Some(tile)
		}))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		// Yield only coords that the source's pyramid actually covers — for a
		// small dataset at high `max_zoom`, this avoids reporting millions of
		// empty coords through the pipeline.
		let pyramid = self.metadata.tile_pyramid();
		Ok(TileStream::from_iter_coord(
			bbox.into_iter_coords(),
			move |coord| match &pyramid {
				Some(p) if !p.includes_coord(&coord) => None,
				_ => Some(()),
			},
		))
	}
}

/// Populate `tilejson.vector_layers` with a single entry describing this
/// import's layer (id, fields, zoom range). MBTiles consumers (QGIS, Mapbox
/// GL, etc.) read this to discover what's inside the tiles.
fn populate_vector_layers(tilejson: &mut TileJSON, layer_name: &str, import: &FeatureImport) {
	set_single_vector_layer(
		tilejson,
		layer_name,
		import.property_schema().clone(),
		import.min_zoom(),
		import.max_zoom(),
	);
}

#[cfg(test)]
mod tests {
	use geo_types::{Geometry, Point};
	use versatiles_core::TileBBox;
	use versatiles_geometry::{
		feature_import::{FeatureImportArgs, project_and_flatten},
		geo::GeoValue,
	};

	use super::*;

	// ── apply_property_filters ───────────────────────────────────────────

	fn feature_with(props: &[(&str, &str)]) -> GeoFeature {
		let mut f = GeoFeature::new(Geometry::Point(Point::new(0.0, 0.0)));
		for (k, v) in props {
			f.set_property((*k).into(), *v);
		}
		f
	}

	fn prop_keys(f: &GeoFeature) -> Vec<String> {
		let mut keys: Vec<String> = f.properties.iter().map(|(k, _)| k.clone()).collect();
		keys.sort();
		keys
	}

	#[test]
	fn apply_property_filters_include_drops_unlisted_keys() {
		let mut features = vec![feature_with(&[("keep", "1"), ("drop", "2"), ("also_drop", "3")])];
		let keep = vec!["keep".to_string()];
		apply_property_filters(&mut features, Some(&keep), None);
		assert_eq!(prop_keys(&features[0]), vec!["keep"]);
	}

	#[test]
	fn apply_property_filters_exclude_drops_listed_keys() {
		let mut features = vec![feature_with(&[("a", "1"), ("b", "2"), ("c", "3")])];
		let drop = vec!["b".to_string()];
		apply_property_filters(&mut features, None, Some(&drop));
		assert_eq!(prop_keys(&features[0]), vec!["a", "c"]);
	}

	#[test]
	fn apply_property_filters_neither_is_noop() {
		let mut features = vec![feature_with(&[("a", "1"), ("b", "2")])];
		apply_property_filters(&mut features, None, None);
		assert_eq!(prop_keys(&features[0]), vec!["a", "b"]);
	}

	#[test]
	fn apply_property_filters_include_precedence_when_both_are_set() {
		// The callers reject `Some + Some` at the VPL parsing layer; the
		// helper itself defines include as winning. Documented behaviour.
		let mut features = vec![feature_with(&[("a", "1"), ("b", "2")])];
		let keep = vec!["a".to_string()];
		let drop = vec!["a".to_string()];
		apply_property_filters(&mut features, Some(&keep), Some(&drop));
		assert_eq!(prop_keys(&features[0]), vec!["a"], "include must win");
	}

	// ── FeatureTileSource construction + accessors ───────────────────────

	fn point_feature(id: u64, name: &str, lon: f64, lat: f64) -> GeoFeature {
		let mut f = GeoFeature::new(Geometry::Point(Point::new(lon, lat)));
		f.set_id(GeoValue::from(id));
		f.set_property("name".into(), name);
		f
	}

	fn build_source(max_zoom: u8) -> FeatureTileSource {
		build_source_with_cap(max_zoom, None)
	}

	fn build_source_with_cap(max_zoom: u8, max_tile_bytes: Option<MaxTileBytes>) -> FeatureTileSource {
		let features: Vec<GeoFeature> = vec![
			point_feature(1, "origin", 0.0, 0.0),
			point_feature(2, "east", 90.0, 30.0),
		]
		.into_iter()
		.flat_map(project_and_flatten)
		.collect();
		let args = FeatureImportArgs {
			max_zoom: Some(max_zoom),
			..Default::default()
		};
		let import = FeatureImport::from_features(features, args).unwrap();
		FeatureTileSource::new(
			import,
			"features",
			TileCompression::Uncompressed,
			"test_label",
			"test source",
			"test",
			max_tile_bytes,
		)
		.unwrap()
	}

	#[test]
	fn new_populates_metadata_and_tilejson() {
		let source = build_source(2);
		// metadata accessor + tile_pyramid present
		let m = source.metadata();
		assert_eq!(m.tile_format(), &TileFormat::MVT);
		assert_eq!(m.tile_compression(), &TileCompression::Uncompressed);
		assert!(m.tile_pyramid().is_some(), "pyramid set after construction");

		// tilejson has the layer name + a single vector_layers entry
		let tj = source.tilejson();
		assert_eq!(tj.vector_layers.0.len(), 1);
		assert!(tj.vector_layers.0.contains_key("features"));
	}

	#[test]
	fn debug_impl_emits_struct_name_and_source_short() {
		let source = build_source(2);
		let s = format!("{source:?}");
		assert!(s.contains("FeatureTileSource"), "got: {s}");
		assert!(s.contains("test"), "should expose source_short, got: {s}");
	}

	#[test]
	fn source_type_uses_descriptor_strings() {
		let source = build_source(2);
		let st = source.source_type().to_string();
		assert!(st.contains("test source"), "got: {st}");
		assert!(st.contains("test"), "got: {st}");
	}

	#[tokio::test]
	async fn tile_pyramid_async_accessor_returns_set_pyramid() -> Result<()> {
		let source = build_source(2);
		let pyramid = source.tile_pyramid().await?;
		// At max_zoom=2 the world is 4×4 tiles; our two points span lon
		// 0..90°, lat 0..30°, so the level-2 bbox is a real (non-empty) range.
		assert!(pyramid.level_max().is_some());
		Ok(())
	}

	// ── tile / tile_stream / tile_coord_stream ───────────────────────────

	#[tokio::test]
	async fn tile_returns_some_for_world_tile() -> Result<()> {
		let source = build_source(2);
		let coord = TileCoord::new(0, 0, 0)?;
		let tile = source.tile(&coord).await?;
		assert!(tile.is_some(), "world tile must be non-empty for non-empty input");
		Ok(())
	}

	#[tokio::test]
	async fn tile_returns_none_outside_data_bbox() -> Result<()> {
		let source = build_source(2);
		// Tile (2, 0, 3) is the bottom-left z2 tile, covering southern latitudes;
		// our input is in the northern hemisphere, so the import returns None.
		let coord = TileCoord::new(2, 0, 3)?;
		let tile = source.tile(&coord).await?;
		assert!(tile.is_none(), "tile outside data must be None");
		Ok(())
	}

	#[tokio::test]
	async fn tile_errors_when_above_configured_hard_cap() -> Result<()> {
		// A 1-byte cap makes every rendered tile over-cap: the single-tile API
		// must propagate the error rather than drop the tile.
		let source = build_source_with_cap(2, Some(MaxTileBytes::Limit(1)));
		let coord = TileCoord::new(0, 0, 0)?;
		let err = source.tile(&coord).await.unwrap_err();
		assert!(format!("{err:#}").contains("hard cap"), "{err:#}");
		Ok(())
	}

	#[tokio::test]
	async fn tile_passes_when_hard_cap_disabled() -> Result<()> {
		// `max_tile_bytes=none` disables the hard cap: the same tile that
		// errored under a 1-byte cap is emitted normally.
		let source = build_source_with_cap(2, Some(MaxTileBytes::Disabled));
		let coord = TileCoord::new(0, 0, 0)?;
		let tile = source.tile(&coord).await?;
		assert!(tile.is_some(), "disabled hard cap must emit the tile");
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_obeys_configured_hard_cap() -> Result<()> {
		// The streaming path skips over-cap tiles (returning None), so a 1-byte
		// cap must yield nothing while a disabled cap yields the data tiles.
		let capped = build_source_with_cap(0, Some(MaxTileBytes::Limit(1)));
		let bbox = TileBBox::new_full(0)?;
		let mut stream = capped.tile_stream(bbox).await?;
		assert!(
			stream.next().await.is_none(),
			"all tiles over a 1-byte cap must be skipped"
		);

		let uncapped = build_source_with_cap(0, Some(MaxTileBytes::Disabled));
		let mut stream = uncapped.tile_stream(bbox).await?;
		assert!(stream.next().await.is_some(), "disabled hard cap must emit the tile");
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_yields_tiles_for_world_bbox() -> Result<()> {
		let source = build_source(0);
		let bbox = TileBBox::new_full(0)?;
		let mut stream = source.tile_stream(bbox).await?;
		let mut count = 0;
		while let Some(_item) = stream.next().await {
			count += 1;
		}
		assert_eq!(count, 1, "expected exactly the world tile (0,0,0)");
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_skips_pyramid_tiles_with_no_features() -> Result<()> {
		// At z=3, the two points sit in tiles widely separated along the
		// x-axis. The pyramid bbox covers the whole rectangle between them, so
		// the in-between columns are empty and hit the `Ok(None)` branch in
		// `import.get_tile(..)`.
		let source = build_source(3);
		let pyramid = source.tile_pyramid().await?;
		let level = pyramid.level_max().unwrap();
		let bbox = pyramid.level_ref(level).to_bbox();
		let total = bbox.count_tiles();
		let mut stream = source.tile_stream(bbox).await?;
		let mut yielded = 0u64;
		while let Some(_t) = stream.next().await {
			yielded += 1;
		}
		assert!(yielded > 0, "at least one data tile must be present");
		assert!(
			yielded < total,
			"tile_stream must filter out data-empty tiles (yielded {yielded} of {total})",
		);
		Ok(())
	}

	#[tokio::test]
	async fn tile_coord_stream_filters_to_pyramid_bbox() -> Result<()> {
		let source = build_source(2);
		// Pass a full z2 bbox; the pyramid only covers the data's slice of
		// z2, so `tile_coord_stream` must filter out tiles outside that slice.
		let bbox = TileBBox::new_full(2)?;
		let mut stream = source.tile_coord_stream(bbox).await?;
		let mut count = 0;
		while let Some(_item) = stream.next().await {
			count += 1;
		}
		// Full z2 bbox has 16 tiles; the data covers far fewer. Just assert
		// the filter is doing work (i.e. fewer than 16 yielded).
		assert!(count > 0, "should yield at least one coord");
		assert!(count < 16, "should filter out non-pyramid coords; got {count} of 16");
		Ok(())
	}
}
