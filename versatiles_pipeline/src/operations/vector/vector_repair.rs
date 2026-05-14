//! # Vector Repair Operation
//!
//! Brings every vector tile into MVT 2.1 conformance: polygon ring winding is
//! normalised, degenerate rings are dropped, and polygons whose exteriors
//! would not survive integer-grid quantisation are removed.
//!
//! ## Cost model
//!
//! For each tile:
//!
//! - **Always**: one decode (needed to know whether the tile needs work).
//! - **Only if the tile is dirty**: re-encode every layer through
//!   `VectorTileLayer::from_features`. This is what triggers the encoder's
//!   normalisation + degeneracy filtering.
//!
//! Clean tiles pass through with their original blob intact, so the writer
//! does not have to recompress them.

use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileStream, TileType};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer, validate_tile};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Repairs vector tiles to conform to MVT 2.1: normalises polygon ring
/// winding, drops degenerate rings (collinear / sub-pixel / too-few-vertices),
/// and removes polygons whose exteriors would not survive integer-grid
/// quantisation.
///
/// Tiles that the validator considers clean pass through unchanged — the
/// original encoded blob is forwarded without re-encoding, so this operation
/// is cheap on conformant input.
///
/// ### Example
///
/// ```text
/// from_container filename="bad.versatiles" | vector_repair
/// ```
pub struct Args {}

#[derive(Debug)]
pub struct Operation {
	metadata: TileSourceMetadata,
	source: Arc<Box<dyn TileSource>>,
	tilejson: TileJSON,
}

impl Operation {
	#[context("Building vector_repair operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let _args = Args::from_vpl_node(&vpl_node)?;
		Self::new(source)
	}

	pub fn new(source: Box<dyn TileSource>) -> Result<Operation> {
		ensure!(
			source.metadata().tile_format().to_type() == TileType::Vector,
			"vector_repair requires a vector tile source"
		);

		let metadata = source.as_ref().metadata().clone();
		let tilejson = source.as_ref().tilejson().clone();

		Ok(Self {
			metadata,
			source: Arc::new(source),
			tilejson,
		})
	}
}

/// Returns either the original (untouched) tile if the validator finds nothing
/// to fix, or a rebuilt tile whose layers have been round-tripped through the
/// encoder's normalisation pass.
fn repair_tile_if_needed(mut tile: Tile) -> Result<Tile> {
	let tile_format = tile.format();

	{
		// Scoped borrow: holding `&VectorTile` from `as_vector` would prevent us
		// from moving `tile` later on the clean path.
		let vt = tile.as_vector()?;
		if validate_tile(vt).is_empty() {
			// Clean: drop the borrow and return the tile as-is. Its blob is
			// still present (materialize_content does not clear it), so the
			// writer reuses the original bytes.
			return Ok(tile);
		}
	}

	// Dirty: rebuild every layer's features through from_features so the
	// encoder's winding normalisation and degeneracy filtering fire.
	//
	// Uses the *lenient* feature decoder so that inverted-winding input
	// (the landcover-vectors#3 pattern) is detected per-feature and rewound
	// before classifying outer/inner rings. The strict decoder would drop
	// the original outer rings as "orphan inners" and lose the shape.
	let vt = tile.as_vector()?;
	let mut new_layers: Vec<VectorTileLayer> = Vec::with_capacity(vt.layers.len());
	for layer in &vt.layers {
		let features = layer
			.features
			.iter()
			.map(|f| f.to_feature_lenient(layer))
			.collect::<Result<Vec<_>>>()?;
		new_layers.push(VectorTileLayer::from_features(
			layer.name.clone(),
			features,
			layer.extent,
			layer.version,
		)?);
	}
	let cleaned = VectorTile::new(new_layers);
	Tile::from_vector(cleaned, tile_format)
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
		SourceType::new_processor("vector_repair", self.source.as_ref().source_type())
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.source.tile_pyramid().await
	}

	#[context("Failed to get repaired tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("vector_repair::tile_stream {bbox:?}");
		Ok(self
			.source
			.tile_stream(bbox)
			.await?
			.map_parallel_try(move |_coord, tile| repair_tile_if_needed(tile))
			.unwrap_results())
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let Some(tile) = self.source.tile(coord).await? else {
			return Ok(None);
		};
		Ok(Some(repair_tile_if_needed(tile)?))
	}
}

crate::operations::macros::define_transform_factory!("vector_repair", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use crate::factory::OperationFactoryTrait;
	use crate::helpers::dummy_vector_source::DummyVectorSource;
	use versatiles_container::TileSource;
	use versatiles_core::TilePyramid;

	#[tokio::test]
	async fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "vector_repair");
	}

	#[tokio::test]
	async fn test_factory_docs_mention_repair() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.to_lowercase().contains("repair") || docs.to_lowercase().contains("normalise"));
	}

	#[tokio::test]
	async fn test_rejects_raster_source() {
		use crate::helpers::dummy_image_source::DummyImageSource;
		use versatiles_core::TileFormat;
		let source = Box::new(DummyImageSource::from_color(&[128u8, 0, 0], 4, TileFormat::PNG, None).unwrap());
		let result = Operation::build(
			VPLNode::try_from_str("vector_repair").unwrap(),
			source,
			&PipelineFactory::new_dummy(),
		)
		.await;
		assert!(result.is_err(), "vector_repair must reject raster input");
	}

	#[tokio::test]
	async fn test_build_runs_on_vector_source() -> Result<()> {
		let source = Box::new(DummyVectorSource::new(
			&[("dummy", &[&[("k", "v")]])],
			Some(TilePyramid::new_full_up_to(2)),
		));
		let op = Operation::build(
			VPLNode::try_from_str("vector_repair")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;
		assert!(op.source_type().to_string().contains("vector_repair"));
		Ok(())
	}

	// ── repair_tile_if_needed unit tests ────────────────────────────────
	//
	// Building a deliberately spec-violating MVT tile from outside
	// versatiles_geometry isn't possible without re-exporting `GeomType` —
	// the encoder normalises anything that goes through `from_geometry`.
	// The dirty-input path is covered by the end-to-end test against
	// `berlin.mbtiles` (commit 2), which is known to ship malformed
	// polygons. Here we cover the cheap, common case: clean input passes
	// through without being mangled.

	use geo_types::{Geometry, LineString, Polygon};
	use versatiles_core::TileFormat;
	use versatiles_geometry::geo::GeoFeature;
	use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer};

	fn build_clean_tile() -> Tile {
		// Outer CW in screen (positive surveyor area) = valid MVT 2.1 exterior.
		let outer = LineString::from(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0], [0.0, 0.0]]);
		let poly = Polygon::new(outer, vec![]);
		let feature = GeoFeature::new(Geometry::Polygon(poly));
		let layer = VectorTileLayer::from_features("ok".to_string(), vec![feature], 4096, 1).unwrap();
		Tile::from_vector(VectorTile::new(vec![layer]), TileFormat::MVT).unwrap()
	}

	#[test]
	fn clean_tile_round_trips_without_issues() -> Result<()> {
		let input = build_clean_tile();
		let mut repaired = repair_tile_if_needed(input)?;
		let vt = repaired.as_vector()?;

		// After "repair" (which should be a no-op for clean input), the
		// validator must see zero issues.
		let issues = validate_tile(vt);
		assert!(
			issues.is_empty(),
			"clean input should remain clean after vector_repair, got {issues:?}",
		);
		// And the polygon should still be intact.
		assert_eq!(vt.layers.len(), 1);
		assert_eq!(vt.layers[0].features.len(), 1);
		Ok(())
	}

	/// End-to-end: wrap `vector_repair` around `../testdata/berlin.mbtiles`
	/// and verify the validator reports zero issues on the output. The
	/// fixture is itself the output of an earlier `vector_repair` run, so
	/// this test now mostly exercises the no-op-when-clean path — the
	/// operation should pass every tile through unchanged.
	///
	/// Constructs the operation directly rather than going through the VPL
	/// pipeline factory, which would need the full container registry wired
	/// up just to open an mbtiles file.
	#[tokio::test]
	async fn end_to_end_repairs_berlin_mbtiles() -> Result<()> {
		use versatiles_container::{MBTilesReader, TilesRuntime};

		let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.unwrap()
			.join("testdata/berlin.mbtiles");
		let runtime = TilesRuntime::new_silent();
		let reader = MBTilesReader::open(&path, runtime)?;
		let source: Box<dyn TileSource> = Box::new(reader);
		let op = Operation::new(source)?;

		let pyramid = op.tile_pyramid().await?;
		let mut total_tiles = 0u64;
		let mut total_issues = 0u64;
		let mut sample_issue: Option<String> = None;

		for bbox in pyramid.to_iter_bboxes().filter(|b| !b.is_empty()) {
			let mut stream = op.tile_stream(bbox).await?;
			while let Some((coord, mut tile)) = stream.next().await {
				total_tiles += 1;
				let vt = tile.as_vector()?;
				let issues = validate_tile(vt);
				if !issues.is_empty() {
					total_issues += issues.len() as u64;
					if sample_issue.is_none() {
						sample_issue = Some(format!(
							"z={} x={} y={} layer={:?} feature={} kind={:?}",
							coord.level, coord.x, coord.y, issues[0].layer, issues[0].feature_index, issues[0].kind
						));
					}
				}
			}
		}

		assert!(total_tiles > 0, "expected to walk at least one tile");
		assert_eq!(
			total_issues,
			0,
			"vector_repair must leave no spec issues; first remaining: {}",
			sample_issue.as_deref().unwrap_or("<none>")
		);
		Ok(())
	}
}
