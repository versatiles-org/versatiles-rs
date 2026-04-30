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

use crate::helpers::{
	tile_error_monitor::{TileErrorMonitor, TileErrorStage},
	tile_size_monitor::TileSizeMonitor,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{
	TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream, VectorLayer, VectorLayers,
};
use versatiles_geometry::feature_import::FeatureImport;
use versatiles_geometry::geo::GeoFeature;

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
	pub fn new(
		import: FeatureImport,
		layer_name: &str,
		compression: TileCompression,
		label: &'static str,
		source_description: &'static str,
		source_short: &'static str,
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
			size_monitor: TileSizeMonitor::new(label),
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

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("tile_pyramid not set"))
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		match self.import.get_tile(coord.level, coord.x, coord.y)? {
			Some(vector_tile) => {
				let mut tile = Tile::from_vector(vector_tile, TileFormat::MVT)?;
				tile.change_compression(&self.compression)?;
				let blob = tile.as_blob(&self.compression)?;
				self.size_monitor.check(*coord, blob)?;
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
			if let Err(e) = size_monitor.check(coord, blob) {
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
	let layer = VectorLayer {
		fields: import.property_schema().clone(),
		description: None,
		minzoom: Some(import.min_zoom()),
		maxzoom: Some(import.max_zoom()),
	};
	tilejson.vector_layers = VectorLayers(std::iter::once((layer_name.to_string(), layer)).collect());
}
