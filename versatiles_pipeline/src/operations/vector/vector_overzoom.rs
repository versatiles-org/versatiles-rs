//! # Vector Overzoom Operation
//!
//! This operation generates vector tiles beyond the source's highest zoom level
//! by extracting and re-encoding the relevant sub-region of an existing parent tile -
//! the vector-tile counterpart of `raster_overscale`.
//!
//! ## How It Works
//!
//! Given a tile source with a highest zoom level (e.g., z14), this operation can serve
//! tiles at higher zoom levels (e.g., z15, z16, z17...) by:
//!
//! 1. Finding the corresponding "parent" tile at the source's highest level
//! 2. Computing the child's sub-region in the parent's tile-local coordinate space
//! 3. Clipping every feature to that sub-region (with an optional buffer)
//! 4. Translating + scaling surviving geometry into the child tile's coordinate space
//! 5. Re-encoding the result as a fresh MVT tile
//!
//! For example, a z15 tile covers 1/4 the area of its z14 parent. The operation extracts
//! the corresponding quarter and produces a new tile in which the surviving features fill
//! the full `[0, extent]` range.
//!
//! ## Where it differs from `raster_overscale`
//!
//! `raster_overscale` also fills the gaps a source has at or below its highest level,
//! from the nearest tile below. This operation does not, and never looks further down
//! than the source's highest level: a raster tile that is missing is data the source
//! lacks at that resolution, but a vector tile that is missing is a place with nothing
//! in it — open sea, say. Looking for it further down would mean decoding and clipping
//! a large low-zoom tile for every such place, to find nothing.
//!
//! ## Caching
//!
//! The operation maintains an LRU cache (256MB by default) of decoded source vector tiles.
//! Generating many high-zoom children from the same parent only decodes that parent once.

use std::{fmt::Debug, sync::Arc};

use anyhow::{Result, ensure};
use async_trait::async_trait;
use geo::MapCoords;
use geo_types::{Coord, Geometry, MultiPolygon, Polygon};
use moka::future::Cache;
use versatiles_container::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileJSON, TileStream, ZoomLevel};
use versatiles_derive::context;
use versatiles_geometry::{
	feature_import::clip_geometry,
	geo::{GeoFeature, GeoValue},
	vector_tile::{VectorTile, VectorTileLayer},
};

use crate::{PipelineFactory, operations::removed::refuse_removed_parameters, vpl::VPLNode};

/// Default clip buffer in tile-extent units. Matches tippecanoe's default of 5 pixels
/// at 256-px tile size scaled to the standard MVT extent of 4096 (5 * 4096 / 256 = 80).
const DEFAULT_BUFFER: u32 = 80;
/// Cache budget for decoded parent vector tiles.
const CACHE_CAPACITY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Serves vector tiles above the source's highest zoom level by clipping and rescaling.
///
/// A tile the source has is passed through unchanged. A tile above the source's
/// highest level is clipped out of the source's tile at that level and its
/// coordinates rescaled. No detail is added — the geometry is the parent's —
/// but a client can keep zooming instead of hitting a blank map. A tile that
/// would hold no features is not created.
///
/// Nothing is created at or below the source's highest level, and nothing from
/// a tile further down: a vector tile the source lacks is a place with nothing
/// in it. To overzoom from a lower level than the source's highest, put
/// `filter level_max=…` in front.
///
/// `level_max` has to be set above the source's highest level for the
/// operation to have anything to do. Each extra level quadruples the tile
/// count.
pub struct Args {
	/// Highest zoom level to serve, capped at `30`. Defaults to the source's highest.
	pub level_max: Option<ZoomLevel>,

	/// Clip buffer in tile-extent units, so edge-straddling features survive. Defaults to `80`.
	#[vpl(default = "80")]
	pub buffer: Option<u32>,
}

/// The vector overzoom operation state.
#[derive(Clone)]
pub struct Operation {
	metadata: TileSourceMetadata,
	source: SharedTileSource,
	tilejson: TileJSON,
	/// The source's highest level: what every created tile is clipped out of.
	level_top: u8,
	buffer: u32,
	cache: Arc<Cache<TileCoord, Option<Arc<VectorTile>>>>,
}

impl Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Operation")
			.field("metadata", &self.metadata)
			.field("source", &self.source)
			.field("tilejson", &self.tilejson)
			.field("level_top", &self.level_top)
			.field("buffer", &self.buffer)
			.field("cache", &"<moka::future::Cache>")
			.finish()
	}
}

impl Operation {
	#[context("Building vector_overzoom operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		// A parameter that is gone says so, and says what to do.
		refuse_removed_parameters(&vpl_node)?;
		let args = Args::from_vpl_node(&vpl_node)?;
		Self::new(source, args.level_max.map(u8::from), args.buffer)
	}

	/// `level_max` defaults to the source's highest level, as it does for
	/// `raster_overscale` — which here leaves nothing to do, and is refused.
	pub fn new(source: Box<dyn TileSource>, level_max: Option<u8>, buffer: Option<u32>) -> Result<Operation> {
		ensure!(
			source.metadata().tile_format().to_type() == versatiles_core::TileType::Vector,
			"vector_overzoom requires a vector tile source"
		);

		let mut metadata = source.as_ref().metadata().clone();

		let source_pyramid = metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("source tile_pyramid not set"))?;
		let level_top = source_pyramid
			.level_max()
			.ok_or_else(|| anyhow::anyhow!("source pyramid is empty"))?;
		let level_max = level_max.unwrap_or(level_top).min(MAX_ZOOM_LEVEL);

		// Doing nothing without saying so would look like a pipeline that
		// works, until someone zooms in.
		ensure!(
			level_max > level_top,
			"'vector_overzoom' has nothing to do: it creates tiles above the source's highest zoom level, which is {level_top}; set `level_max` to more than that"
		);

		// Above the source's highest level, a tile can exist exactly where the
		// source has one at that level.
		let mut new_pyramid = source_pyramid.as_ref().clone();
		let top = new_pyramid.level_ref(level_top).clone();
		for level in level_top + 1..=level_max {
			new_pyramid.set_level(top.at_level(level));
		}
		metadata.set_tile_pyramid(new_pyramid);

		let mut tilejson = source.as_ref().tilejson().clone();
		metadata.update_tilejson(&mut tilejson);

		let cache = Cache::builder()
			.max_capacity(CACHE_CAPACITY_BYTES)
			.weigher(|_k: &TileCoord, v: &Option<Arc<VectorTile>>| -> u32 {
				// A missing tile costs a cache entry, not nothing: weighed at 8
				// bytes, the budget allowed 32 million of them.
				v.as_ref().map_or(256, |t| {
					// Approximate decoded size by total feature geom_data + tag bytes.
					let bytes: u64 = t
						.layers
						.iter()
						.map(|l| {
							l.features
								.iter()
								.map(|f| f.geom_data.len() + (f.tag_ids.len() as u64) * 4)
								.sum::<u64>()
						})
						.sum();
					u32::try_from(bytes.saturating_add(64)).unwrap_or(u32::MAX)
				})
			})
			.build();
		let cache = Arc::new(cache);

		Ok(Self {
			metadata,
			source: Arc::from(source),
			tilejson,
			level_top,
			buffer: buffer.unwrap_or(DEFAULT_BUFFER),
			cache,
		})
	}

	/// The source's tile at its highest level that covers `coord_dst`, if it
	/// has one there.
	#[context("finding tile for coord {:?}", coord_dst)]
	async fn find_tile(&self, coord_dst: TileCoord) -> Result<Option<(TileCoord, Arc<VectorTile>)>> {
		let coord_src = coord_dst.at_level(self.level_top.min(coord_dst.level));
		Ok(self.try_fetch_tile(coord_src).await?.map(|tile| (coord_src, tile)))
	}

	/// The decoded tile of the source at `coord`. Requests that arrive
	/// together for the same tile share one read and one decode.
	async fn try_fetch_tile(&self, coord: TileCoord) -> Result<Option<Arc<VectorTile>>> {
		self
			.cache
			.try_get_with(coord, async {
				let tile = self.source.tile(&coord).await?;
				tile.map(|t| t.into_vector().map(Arc::new)).transpose()
			})
			.await
			.map_err(|error: Arc<anyhow::Error>| anyhow::anyhow!("{error:#}"))
	}
}

/// Returns `true` once `coords` contains at least `n` integer-grid-distinct vertices,
/// matching the rounding the MVT encoder applies in `write_coord`.
fn has_n_distinct_grid_vertices(coords: &[Coord<f64>], n: usize) -> bool {
	if coords.len() < n {
		return false;
	}
	let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::with_capacity(coords.len());
	for c in coords {
		#[expect(clippy::cast_possible_truncation, reason = "tile-local coordinates, far inside i64")]
		let key = (c.x.round() as i64, c.y.round() as i64);
		seen.insert(key);
		if seen.len() >= n {
			return true;
		}
	}
	false
}

/// Drops geometries whose every ring would round to a degenerate ring on the integer
/// grid (the encoder skips < 3-distinct-vertex rings, producing orphan inners or
/// empty geom_data). Polygons are pruned ring-by-ring; if all rings disappear or the
/// exterior becomes degenerate, the polygon is dropped.
fn drop_invisible(g: Geometry<f64>) -> Option<Geometry<f64>> {
	use geo_types::MultiLineString;
	fn keep_polygon(p: Polygon<f64>) -> Option<Polygon<f64>> {
		let (exterior, interiors) = p.into_inner();
		if !has_n_distinct_grid_vertices(&exterior.0, 3) {
			return None;
		}
		let interiors: Vec<_> = interiors
			.into_iter()
			.filter(|r| has_n_distinct_grid_vertices(&r.0, 3))
			.collect();
		Some(Polygon::new(exterior, interiors))
	}
	match g {
		Geometry::Point(_) | Geometry::MultiPoint(_) => Some(g),
		Geometry::LineString(ls) => {
			if has_n_distinct_grid_vertices(&ls.0, 2) {
				Some(Geometry::LineString(ls))
			} else {
				None
			}
		}
		Geometry::MultiLineString(ml) => {
			let kept: Vec<_> = ml
				.0
				.into_iter()
				.filter(|ls| has_n_distinct_grid_vertices(&ls.0, 2))
				.collect();
			if kept.is_empty() {
				None
			} else {
				Some(Geometry::MultiLineString(MultiLineString(kept)))
			}
		}
		Geometry::Polygon(p) => keep_polygon(p).map(Geometry::Polygon),
		Geometry::MultiPolygon(mp) => {
			let kept: Vec<_> = mp.0.into_iter().filter_map(keep_polygon).collect();
			if kept.is_empty() {
				None
			} else {
				Some(Geometry::MultiPolygon(MultiPolygon(kept)))
			}
		}
		other => Some(other),
	}
}

/// Layer metadata + already-clipped features, prior to MVT re-encoding. Split
/// out of [`extract_tile`] so `tile_coord_stream` can run the (cheap) decode +
/// clip + filter pipeline without paying for re-encoding.
struct ExtractedLayer {
	name: String,
	extent: u32,
	version: u32,
	features: Vec<GeoFeature>,
}

/// Decode + clip + filter half of [`extract_tile`]. Produces the input that
/// [`encode_features`] would feed to `VectorTileLayer::from_features`.
///
/// Returns `Ok(None)` when no features survive the per-child clip. Caller is
/// responsible for the level_diff > 0 invariant — the same-level passthrough
/// path of `extract_tile` short-circuits before reaching this function.
#[context("extracting features for coord {:?}", coord_dst)]
fn extract_features(
	tile_src: &VectorTile,
	coord_src: TileCoord,
	coord_dst: TileCoord,
	buffer: u32,
) -> Result<Option<Vec<ExtractedLayer>>> {
	let level_diff = i32::from(coord_dst.level) - i32::from(coord_src.level);
	ensure!(level_diff > 0, "extract_features requires level_diff > 0");

	#[expect(clippy::cast_sign_loss, reason = "`level_diff` is a small positive zoom difference")]
	let scale = 1u32 << level_diff as u32;
	let f_scale = f64::from(scale);
	let buf = f64::from(buffer);

	let mut out_layers: Vec<ExtractedLayer> = Vec::new();
	for layer_src in &tile_src.layers {
		if layer_src.extent == Some(0) {
			continue;
		}
		let extent = layer_src.extent.unwrap_or(4096);
		let sub_extent = f64::from(extent) / f_scale;

		// Sub-tile within the parent that corresponds to coord_dst.
		// `coord_dst.{x,y} % scale` selects the sub-cell in the (parent-relative) grid.
		let offset_x = f64::from(coord_dst.x % scale) * sub_extent;
		let offset_y = f64::from(coord_dst.y % scale) * sub_extent;

		// convert to parent extent units so the clip happens in parent space
		let buf_p = buf / f_scale;
		let bbox = [
			offset_x - buf_p,
			offset_y - buf_p,
			offset_x + sub_extent + buf_p,
			offset_y + sub_extent + buf_p,
		];

		let mut out_features: Vec<GeoFeature> = Vec::new();
		for feature_src in &layer_src.features {
			let geometry = match feature_src.to_geometry() {
				Ok(g) => g,
				Err(e) => {
					log::trace!("skipping unreadable feature: {e:#}");
					continue;
				}
			};
			let id = feature_src.id.map(GeoValue::from);
			let properties = feature_src.decode_properties(layer_src)?;

			for piece in clip_geometry(geometry, bbox) {
				let mapped = piece.map_coords(|c| Coord {
					x: (c.x - offset_x) * f_scale,
					y: (c.y - offset_y) * f_scale,
				});

				let Some(mapped) = drop_invisible(mapped) else { continue };
				out_features.push(GeoFeature {
					id: id.clone(),
					geometry: mapped,
					properties: properties.clone(),
				});
			}
		}

		if out_features.is_empty() {
			continue;
		}

		out_layers.push(ExtractedLayer {
			name: layer_src.name.clone(),
			extent,
			version: layer_src.version.unwrap_or(1),
			features: out_features,
		});
	}

	if out_layers.is_empty() {
		Ok(None)
	} else {
		Ok(Some(out_layers))
	}
}

/// Re-encode half: builds a [`VectorTile`] from extracted layers by running
/// each through `VectorTileLayer::from_features`.
fn encode_features(layers: Vec<ExtractedLayer>) -> Result<VectorTile> {
	let out_layers = layers
		.into_iter()
		.map(|l| VectorTileLayer::from_features(l.name, l.features, l.extent, l.version))
		.collect::<Result<Vec<_>>>()?;
	Ok(VectorTile::new(out_layers))
}

/// Extracts and rescales features from a parent vector tile to produce a child tile.
///
/// For each layer in `tile_src`, every feature is decoded to `Geometry<f64>`, clipped to
/// the child's sub-region (expanded by `buffer` extent units on every side), and the
/// surviving pieces are translated + scaled into the child tile's coordinate space. The
/// resulting layer keeps the source layer's `extent` and `version`.
///
/// Returns `Ok(None)` when no surviving features remain (e.g. the child tile covers an
/// empty quadrant of the parent).
#[context("extracting tile for coord {:?}", coord_dst)]
fn extract_tile(
	tile_src: &VectorTile,
	coord_src: TileCoord,
	coord_dst: TileCoord,
	buffer: u32,
) -> Result<Option<VectorTile>> {
	let level_diff = i32::from(coord_dst.level) - i32::from(coord_src.level);
	ensure!(level_diff >= 0, "difference in levels must be non-negative");

	if level_diff == 0 {
		return Ok(Some(tile_src.clone()));
	}

	match extract_features(tile_src, coord_src, coord_dst, buffer)? {
		Some(layers) => Ok(Some(encode_features(layers)?)),
		None => Ok(None),
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("vector_overzoom", self.source.as_ref().source_type())
	}

	#[context("Failed to get stream for bbox: {:?}", bbox_dst)]
	async fn tile_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("vector_overzoom::tile_stream {bbox_dst:?}");

		if let Some(pyramid) = self.metadata.tile_pyramid()
			&& !pyramid.intersects_bbox(&bbox_dst)
		{
			return Ok(TileStream::empty());
		}

		if bbox_dst.level() <= self.level_top {
			return self.source.as_ref().tile_stream(bbox_dst).await;
		}

		let self_arc = Arc::new(self.clone());
		let buffer = self.buffer;
		let tile_format = *self.metadata.tile_format();

		let stream = TileStream::from_bbox_async_parallel(bbox_dst, move |coord_dst| {
			let self_arc = self_arc.clone();
			async move {
				match self_arc.find_tile(coord_dst).await {
					Ok(Some((coord_src, tile_src))) => match extract_tile(&tile_src, coord_src, coord_dst, buffer) {
						Ok(Some(vt)) => match Tile::from_vector(vt, tile_format) {
							Ok(tile) => Some((coord_dst, tile)),
							Err(e) => {
								log::error!("Error creating tile {coord_dst:?}: {e:?}");
								None
							}
						},
						Ok(None) => None,
						Err(e) => {
							log::error!("Error extracting tile {coord_dst:?}: {e:?}");
							None
						}
					},
					Ok(None) => None,
					Err(e) => {
						log::error!("Error processing tile {coord_dst:?}: {e:?}");
						None
					}
				}
			}
		});

		Ok(stream)
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if let Some(pyramid) = self.metadata.tile_pyramid()
			&& !pyramid.intersects_bbox(&bbox)
		{
			return Ok(TileStream::empty());
		}
		if bbox.level() <= self.level_top {
			return self.source.as_ref().tile_coord_stream(bbox).await;
		}

		// To honour the `TileSource` stream-count invariant we have to run the
		// same decode + clip + filter pipeline as `tile_stream` and only emit
		// coords whose features survive. We skip the (expensive) MVT re-encode
		// by stopping at `extract_features` — saves ~50% of the per-tile cost
		// while keeping counts exact. Source tiles are still served from the
		// per-operation cache, so the decode happens at most once per source
		// tile across both stream calls.
		let self_arc = Arc::new(self.clone());
		let buffer = self.buffer;

		let stream = TileStream::from_bbox_async_parallel(bbox, move |coord_dst| {
			let self_arc = self_arc.clone();
			async move {
				match self_arc.find_tile(coord_dst).await {
					Ok(Some((coord_src, tile_src))) => match extract_features(&tile_src, coord_src, coord_dst, buffer) {
						Ok(Some(_)) => Some((coord_dst, ())),
						Ok(None) => None,
						Err(e) => {
							log::error!("Error counting tile {coord_dst:?}: {e:?}");
							None
						}
					},
					Ok(None) => None,
					Err(e) => {
						log::error!("Error finding tile {coord_dst:?}: {e:?}");
						None
					}
				}
			}
		});
		Ok(stream)
	}
}

crate::operations::macros::define_transform_factory!("vector_overzoom", Args, Operation, requires: Vector);

#[cfg(test)]
mod tests {
	use geo_types::Geometry;
	use versatiles_core::{TileBBox, TileCoord, TilePyramid};
	use versatiles_geometry::geo::GeoValue;

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_vector_source::DummyVectorSource};

	/// A source with every tile up to level 2, overzoomed to `level_max`.
	async fn build_op(level_max: u8) -> Result<Operation> {
		let source = Box::new(DummyVectorSource::new(
			&[("dummy", &[&[("k", "v")]])],
			Some(TilePyramid::new_full_up_to(2)),
		));
		Operation::build(
			VPLNode::try_from_str(&format!("vector_overzoom level_max={level_max}"))?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	#[tokio::test]
	async fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "vector_overzoom");
	}

	#[tokio::test]
	async fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("level_max"));
		assert!(docs.contains("buffer"));
		assert!(!docs.contains("level_base"));
		assert!(!docs.contains("enable_climbing"));
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = build_op(6).await?;
		assert!(op.source_type().to_string().contains("vector_overzoom"));
		Ok(())
	}

	#[tokio::test]
	async fn test_pyramid_extended() -> Result<()> {
		let op = build_op(5).await?;
		let pyramid = op.metadata.tile_pyramid().unwrap();
		assert_eq!(pyramid.level_max(), Some(5));
		assert_eq!(
			pyramid.level_ref(5).count_tiles(),
			1024,
			"every tile over a tile of level 2"
		);
		assert_eq!(op.tilejson().zoom_max(), Some(5));
		Ok(())
	}

	/// `level_max` defaults to the source's highest level, as for
	/// `raster_overscale`. Here that leaves nothing to do, and a pipeline
	/// that silently did nothing would look like one that works.
	#[rstest::rstest]
	#[case::not_given("vector_overzoom")]
	#[case::at_the_highest_level("vector_overzoom level_max=2")]
	#[case::below_it("vector_overzoom level_max=1")]
	#[tokio::test]
	async fn refuses_when_there_is_nothing_to_do(#[case] vpl: &str) {
		let source = Box::new(DummyVectorSource::new(
			&[("dummy", &[&[("k", "v")]])],
			Some(TilePyramid::new_full_up_to(2)),
		));
		let error = Operation::build(
			VPLNode::try_from_str(vpl).unwrap(),
			source,
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap_err();
		let message = format!("{error:#}");
		assert!(message.contains("nothing to do"), "{message}");
		assert!(message.contains("which is 2"), "{message}");
	}

	#[rstest::rstest]
	#[case("level_base=2", "filter level_max")]
	#[case("enable_climbing=true", "remove the parameter")]
	#[tokio::test]
	async fn a_removed_parameter_says_what_to_do_instead(#[case] parameter: &str, #[case] advice: &str) {
		let source = Box::new(DummyVectorSource::new(
			&[("dummy", &[&[("k", "v")]])],
			Some(TilePyramid::new_full_up_to(2)),
		));
		let error = Operation::build(
			VPLNode::try_from_str(&format!("vector_overzoom level_max=4 {parameter}")).unwrap(),
			source,
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap_err();
		let message = format!("{error:#}");
		assert!(message.contains("no longer has the parameter"), "{message}");
		assert!(message.contains(advice), "{message}");
	}

	#[tokio::test]
	async fn test_level_below_base_passthrough() -> Result<()> {
		let op = build_op(6).await?;
		let bbox = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);
		// Passed through: should not be re-encoded with the overzoom path.
		Ok(())
	}

	#[tokio::test]
	async fn test_overzoom_generates_child_tile() -> Result<()> {
		// DummyVectorSource emits one MVT-local Point at (1, 2) per source tile,
		// plus properties `x`/`y`/`z` carrying the source coord and `k="v"`.
		// For destination (3, 2, 2) the parent at the source's highest level is (2, 1, 1); the
		// child sub-region within that parent is its top-left quadrant
		// (offset_x = offset_y = 0, scale = 2). The point sits inside the
		// sub-region and rescales to (2, 4) in child space.
		let op = build_op(6).await?;
		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "exactly one child tile expected");

		let (coord, mut tile) = tiles.into_iter().next().unwrap();
		assert_eq!((coord.level, coord.x, coord.y), (3, 2, 2));

		let vt = tile.as_vector()?;
		assert_eq!(vt.layers.len(), 1, "single layer carried through");
		let layer = &vt.layers[0];
		assert_eq!(layer.name, "dummy");
		assert_eq!(layer.extent, Some(4096), "extent preserved from source layer");
		assert_eq!(layer.features.len(), 1, "single feature carried through");

		let feature = &layer.features[0];
		let pt = match feature.to_geometry()? {
			Geometry::MultiPoint(mp) => {
				assert_eq!(mp.0.len(), 1, "single point round-trips as one-element MultiPoint");
				mp.0[0].0
			}
			other => panic!("expected MultiPoint, got {other:?}"),
		};
		// Rescaled child-space coords. MVT integer-grid rounding makes these
		// exact: 1 → 2, 2 → 4 after the ×2 scale.
		assert!((pt.x - 2.0).abs() < 0.5, "child x should be 2.0, got {}", pt.x);
		assert!((pt.y - 4.0).abs() < 0.5, "child y should be 4.0, got {}", pt.y);

		// Properties are passed through unchanged from the source tile at z=2 (1, 1).
		let props = feature.decode_properties(layer)?;
		assert_eq!(props.get("k"), Some(&GeoValue::String("v".to_string())));
		assert_eq!(props.get("z"), Some(&GeoValue::UInt(2)));
		assert_eq!(props.get("x"), Some(&GeoValue::UInt(1)));
		assert_eq!(props.get("y"), Some(&GeoValue::UInt(1)));
		Ok(())
	}

	/// A source whose highest level, 1, has three of its four tiles. The tile
	/// `(1, 1, 1)` is missing, though the tile below it at level 0 exists.
	async fn build_sparse_op() -> Result<Operation> {
		let pyramid = TilePyramid::from_tile_coords(
			[
				TileCoord::new(0, 0, 0)?,
				TileCoord::new(1, 0, 0)?,
				TileCoord::new(1, 1, 0)?,
				TileCoord::new(1, 0, 1)?,
				// (1, 1, 1) deliberately omitted.
			]
			.into_iter(),
		);
		let source = Box::new(DummyVectorSource::new(&[("dummy", &[&[("k", "v")]])], Some(pyramid)));
		Operation::build(
			VPLNode::try_from_str("vector_overzoom level_max=3")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	#[tokio::test]
	async fn a_tile_is_clipped_out_of_the_highest_level_of_the_source() -> Result<()> {
		// Destination (2, 0, 0): its parent at level 1 is (1, 0, 0), which the
		// source has.
		let op = build_sparse_op().await?;
		let (coord_src, _) = op
			.find_tile(TileCoord::new(2, 0, 0)?)
			.await?
			.expect("the source has it");
		assert_eq!((coord_src.level, coord_src.x, coord_src.y), (1, 0, 0));
		Ok(())
	}

	/// Where the source has no tile at its highest level, there is nothing:
	/// a vector tile that is missing is an empty place. The tile at level 0
	/// covers it too, and is not looked at.
	#[tokio::test]
	async fn nothing_is_created_from_below_the_highest_level() -> Result<()> {
		let op = build_sparse_op().await?;
		// Destination (2, 2, 2): its parent at level 1 is the missing (1, 1, 1).
		assert!(op.find_tile(TileCoord::new(2, 2, 2)?).await?.is_none());

		let bbox = TileCoord::new(2, 2, 2)?.to_tile_bbox();
		assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), 0);
		assert_eq!(op.tile_coord_stream(bbox).await?.to_vec().await.len(), 0);

		// Next to it, over a tile the source has, there is one.
		let bbox = TileCoord::new(2, 0, 0)?.to_tile_bbox();
		assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), 1);

		// And the pyramid does not announce the gap.
		let pyramid = op.metadata.tile_pyramid().unwrap();
		assert_eq!(pyramid.level_ref(2).count_tiles(), 12);
		assert!(!pyramid.includes_coord(&TileCoord::new(2, 2, 2)?));
		Ok(())
	}

	#[tokio::test]
	async fn satisfies_stream_count_invariant() -> Result<()> {
		// Exact counts across both streams — the override implementation routes
		// `tile_coord_stream` through `extract_features` so destinations whose
		// features don't survive the per-child clip are dropped from the count
		// too.
		let op = build_op(6).await?;
		let pyr = op.tile_pyramid().await?;
		for z in 0..=pyr.level_max().unwrap_or(0) {
			let bbox = pyr.level_ref(z).to_bbox();
			if bbox.is_empty() {
				continue;
			}
			versatiles_container::testing::assert_stream_counts_agree(&op, bbox).await?;
		}
		Ok(())
	}

	#[tokio::test]
	async fn output_tiles_pass_mvt_validation() -> Result<()> {
		use crate::helpers::assert_tiles_valid;
		let op = build_op(6).await?;
		// the source ends at z=2; request tiles at z=3 so the overzoom path is exercised
		let bbox = TileBBox::from_min_and_max(3, 0, 0, 3, 3)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}

	#[tokio::test]
	async fn test_rejects_raster_source() {
		use versatiles_core::TileFormat;

		use crate::helpers::dummy_image_source::DummyImageSource;
		let source = Box::new(DummyImageSource::from_color(&[128u8, 0, 0], 4, TileFormat::PNG, None).unwrap());
		let result = Operation::build(
			VPLNode::try_from_str("vector_overzoom level_max=4").unwrap(),
			source,
			&PipelineFactory::new_dummy(),
		)
		.await;
		assert!(result.is_err());
	}

	// ── extract_tile unit tests ──────────────────────────────────────────────

	use geo_types::{LineString, Point};
	use versatiles_geometry::geo::GeoFeature as GF;

	fn layer_with_feature(name: &str, geometry: Geometry<f64>) -> VectorTileLayer {
		let f = GF::new(geometry);
		VectorTileLayer::from_features(name.to_string(), vec![f], 4096, 1).unwrap()
	}

	#[test]
	fn extract_same_level_clones() -> Result<()> {
		let pt = Geometry::Point(Point::new(100.0, 100.0));
		let tile = VectorTile::new(vec![layer_with_feature("L", pt)]);
		let coord = TileCoord::new(2, 1, 1)?;
		let out = extract_tile(&tile, coord, coord, 0)?.unwrap();
		assert_eq!(out.layers.len(), 1);
		Ok(())
	}

	#[test]
	fn extract_drops_features_outside_subregion() -> Result<()> {
		// At z2 the tile spans [0..4096]^2 (extent). When extracting the top-left z3
		// child (sub-x=0, sub-y=0), only features inside parent-coords [0..2048]^2 survive
		// (modulo a small buffer). A point at (3000, 3000) lies outside.
		let pt = Geometry::Point(Point::new(3000.0, 3000.0));
		let tile = VectorTile::new(vec![layer_with_feature("L", pt)]);
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 0, 0)?;
		let out = extract_tile(&tile, parent, child, 0)?;
		assert!(out.is_none(), "feature outside child sub-region must be dropped");
		Ok(())
	}

	#[test]
	fn extract_keeps_and_rescales_inside_features() -> Result<()> {
		// Point at parent coords (512, 512) - inside the top-left z3 quarter (<= 2048).
		// After extraction it should land at child coords (1024, 1024), scaled x2.
		let pt = Geometry::Point(Point::new(512.0, 512.0));
		let tile = VectorTile::new(vec![layer_with_feature("L", pt)]);
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 0, 0)?;
		let out = extract_tile(&tile, parent, child, 0)?.unwrap();
		let layer = &out.layers[0];
		assert_eq!(layer.features.len(), 1);

		let geom = layer.features[0].to_geometry()?;
		// Feature decodes as MultiPoint (round-trips Point -> MultiPoint)
		let pt_out = match geom {
			Geometry::MultiPoint(mp) => mp.0[0],
			other => panic!("expected MultiPoint, got {other:?}"),
		};
		assert!((pt_out.x() - 1024.0).abs() < 1.0);
		assert!((pt_out.y() - 1024.0).abs() < 1.0);
		Ok(())
	}

	/// helper - build a CW (screen) ring as a closed LineString
	fn ring_cw(verts: &[(f64, f64)]) -> geo_types::LineString<f64> {
		let mut v: Vec<Coord<f64>> = verts.iter().map(|&(x, y)| Coord { x, y }).collect();
		if v.first() != v.last() {
			v.push(v[0]);
		}
		geo_types::LineString::new(v)
	}

	#[test]
	fn extract_polygon_with_hole_round_trip() -> Result<()> {
		// Simulate a parent z2 tile with a polygon (extent 4096) that has a hole.
		// MVT spec: outer rings are CW in screen coords (positive surveyor area2);
		// inner rings (holes) are CCW (negative area2).
		let outer = ring_cw(&[(0.0, 0.0), (4096.0, 0.0), (4096.0, 4096.0), (0.0, 4096.0)]);
		// Hole at (1000..3000, 1000..3000), wound CCW in screen.
		let inner = ring_cw(&[(1000.0, 1000.0), (1000.0, 3000.0), (3000.0, 3000.0), (3000.0, 1000.0)]);
		let polygon = geo_types::Polygon::new(outer, vec![inner]);
		let tile = VectorTile::new(vec![layer_with_feature("L", Geometry::Polygon(polygon))]);

		// Sanity: source feature decodes as a polygon-with-hole.
		let src_geom = tile.layers[0].features[0].to_geometry()?;
		match &src_geom {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "source: one polygon");
				assert_eq!(mp.0[0].interiors().len(), 1, "source: one hole");
			}
			other => panic!("source: expected MultiPolygon, got {other:?}"),
		}

		// Extract child z3 (1,1) — middle-bottom-right quarter (sub-x=1,sub-y=1).
		// In parent coords that's [2048..4096, 2048..4096] — overlaps outer & corner of hole.
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 1, 1)?;
		let out = extract_tile(&tile, parent, child, 0)?.expect("output tile exists");
		assert_eq!(out.layers.len(), 1);
		let f = &out.layers[0].features[0];
		// Decode and verify the polygon-with-hole survived.
		let out_geom = f.to_geometry()?;
		match &out_geom {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "output: still a single polygon");
				assert_eq!(
					mp.0[0].interiors().len(),
					1,
					"output: hole should survive (got {} interiors)",
					mp.0[0].interiors().len()
				);
			}
			other => panic!("output: expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	#[test]
	fn extract_polygon_when_bbox_inside_hole() -> Result<()> {
		// Bbox is entirely inside the hole -> exterior clips to bbox, interior also clips
		// to bbox. Renderers must see them with opposite orientations so the bbox cancels.
		let outer = ring_cw(&[(0.0, 0.0), (4096.0, 0.0), (4096.0, 4096.0), (0.0, 4096.0)]);
		let inner = ring_cw(&[(500.0, 500.0), (500.0, 3500.0), (3500.0, 3500.0), (3500.0, 500.0)]);
		let polygon = geo_types::Polygon::new(outer, vec![inner]);
		let tile = VectorTile::new(vec![layer_with_feature("L", Geometry::Polygon(polygon))]);

		// Child z3 (0,0): parent coords [0..2048, 0..2048]. The hole [500..3500] contains
		// most of this — specifically the sub-region (500..2048) in both axes. So the
		// child overlaps both interior-of-outer and inside-of-hole.
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 0, 0)?;
		let out = extract_tile(&tile, parent, child, 0)?.expect("output tile exists");
		let f = &out.layers[0].features[0];
		// Inspect ring count by parsing geom_data directly (since to_geometry would
		// already enforce strict ordering / classify them).
		let mut reader = versatiles_core::io::ValueReaderSlice::new_le(f.geom_data.as_slice());
		use versatiles_core::io::ValueReader;
		let mut ring_count = 0usize;
		while reader.has_remaining()? {
			let v = reader.read_varint()?;
			match v & 0x7 {
				1 => {
					ring_count += 1;
					let n = v >> 3;
					for _ in 0..n {
						let _ = reader.read_svarint()?;
						let _ = reader.read_svarint()?;
					}
				}
				2 => {
					let n = v >> 3;
					for _ in 0..n {
						let _ = reader.read_svarint()?;
						let _ = reader.read_svarint()?;
					}
				}
				7 => {}
				other => panic!("unexpected MVT command {other}"),
			}
		}
		assert_eq!(ring_count, 2, "expected outer + inner ring on output");
		Ok(())
	}

	#[test]
	fn extract_clips_lines_at_subregion_boundary() -> Result<()> {
		// Horizontal line spanning the full width of the parent at y=512. The top-left
		// z3 child (parent coords [0..2048]^2) should keep the segment from x=0..2048 only;
		// after rescale, that becomes x=0..4096 in child space.
		let line = Geometry::LineString(LineString::from(vec![[0.0, 512.0], [4096.0, 512.0]]));
		let tile = VectorTile::new(vec![layer_with_feature("L", line)]);
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 0, 0)?;
		let out = extract_tile(&tile, parent, child, 0)?.unwrap();
		let geom = out.layers[0].features[0].to_geometry()?;
		// Decoded as MultiLineString (single-linestring round-trips that way).
		let coords: Vec<_> = match geom {
			Geometry::MultiLineString(ml) => ml.0[0].0.clone(),
			other => panic!("expected MultiLineString, got {other:?}"),
		};
		// Endpoints should be at child-x = 0 and child-x = 4096 (the boundary), child-y = 1024.
		assert!((coords.first().unwrap().x - 0.0).abs() < 1.0);
		assert!((coords.last().unwrap().x - 4096.0).abs() < 1.0);
		for c in &coords {
			assert!((c.y - 1024.0).abs() < 1.0);
		}
		Ok(())
	}

	/// Pulls out the first feature of the first layer and returns its coordinates,
	/// asserting the geometry is a single line.
	fn first_line_coords(tile: &VectorTile) -> Result<Vec<Coord<f64>>> {
		let geom = tile.layers[0].features[0].to_geometry()?;
		match geom {
			Geometry::MultiLineString(ml) => Ok(ml.0[0].0.clone()),
			other => panic!("expected MultiLineString, got {other:?}"),
		}
	}

	#[test]
	fn buffer_extends_clip_past_child_edge() -> Result<()> {
		// A line in parent z=2 that crosses the child (3, 0, 0) sub-region edge
		// at parent-x=2048. With buffer=0 the clip stops exactly at the edge;
		// with a positive buffer the clipped piece extends past the visible
		// child-tile boundary by `buffer / scale` parent-units (= `buffer`
		// child-units after rescale).
		let line = Geometry::LineString(LineString::from(vec![[1000.0, 100.0], [3000.0, 100.0]]));
		let tile = VectorTile::new(vec![layer_with_feature("L", line)]);
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 0, 0)?;

		// buffer=0: max-x clipped exactly at the child edge.
		let out_zero = extract_tile(&tile, parent, child, 0)?.expect("non-empty clip");
		let max_x_zero = first_line_coords(&out_zero)?
			.iter()
			.map(|c| c.x)
			.fold(f64::NEG_INFINITY, f64::max);
		assert!(
			(max_x_zero - 4096.0).abs() < 1.0,
			"buffer=0 must clip at child-x=4096, got {max_x_zero}"
		);

		// buffer=80: max-x extends to child-x = 4096 + 80 = 4176.
		let out_buf = extract_tile(&tile, parent, child, 80)?.expect("non-empty clip");
		let max_x_buf = first_line_coords(&out_buf)?
			.iter()
			.map(|c| c.x)
			.fold(f64::NEG_INFINITY, f64::max);
		assert!(
			(max_x_buf - 4176.0).abs() < 1.0,
			"buffer=80 must extend max-x to ~4176 (4096 + buffer), got {max_x_buf}"
		);
		Ok(())
	}

	#[test]
	fn buffer_pulls_in_features_outside_subregion() -> Result<()> {
		// Line entirely outside child (3, 0, 0)'s sub-region [0..2048] but inside
		// the buffer zone — parent-x = [2060..2080]. With buffer=0 the feature
		// is dropped (no surviving pieces, no layer, extract returns None).
		// With buffer=80 (40 parent-units) it survives in the buffer zone.
		let line = Geometry::LineString(LineString::from(vec![[2060.0, 100.0], [2080.0, 100.0]]));
		let tile = VectorTile::new(vec![layer_with_feature("L", line)]);
		let parent = TileCoord::new(2, 0, 0)?;
		let child = TileCoord::new(3, 0, 0)?;

		assert!(
			extract_tile(&tile, parent, child, 0)?.is_none(),
			"buffer=0 must drop a feature entirely outside the sub-region"
		);

		let out = extract_tile(&tile, parent, child, 80)?.expect("buffer=80 keeps the feature");
		let coords = first_line_coords(&out)?;
		assert!(!coords.is_empty(), "buffered feature must produce output coords");
		for c in &coords {
			assert!(
				c.x > 4096.0,
				"buffered coord should sit past the visible tile edge, got x={}",
				c.x
			);
			assert!(
				c.x < 4096.0 + 80.0 + 1.0,
				"buffered coord must not exceed the buffer width, got x={}",
				c.x
			);
		}
		Ok(())
	}
}
