//! # Vector Repair Operation
//!
//! Brings every vector tile into MVT 2.1 conformance by delegating to
//! [`repair_tile`] from `versatiles_geometry`:
//!
//! - Missing `extent` or `version` fields are set to their spec defaults.
//! - Duplicate layer names are collapsed (first layer wins).
//! - Polygon ring winding is normalised; degenerate rings are dropped.
//!
//! By default, features whose geometry cannot be decoded at all are left in
//! place. Set `drop_offenders=true` to have them removed instead.
//!
//! ## Cost model
//!
//! For each tile:
//!
//! - **Always**: one decode + validate pass (needed to detect issues).
//! - **Only if the tile is dirty**: one clone of the decoded tile + re-encode.
//!
//! Clean tiles return the original blob unchanged; no re-encoding, no extra
//! allocation.

use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileStream, TileType};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::repair_tile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Repairs vector tiles to conform to MVT 2.1.
///
/// Always fixed: missing `extent`/`version` fields, duplicate layer names,
/// inverted polygon winding, and degenerate rings.
///
/// Tiles that the validator considers clean pass through unchanged — the
/// original encoded blob is forwarded without re-encoding, so this operation
/// is cheap on conformant input.
///
/// ### Arguments
///
/// - `drop_offenders` (bool, default `false`): when `true`, features whose
///   geometry byte stream cannot be decoded are silently removed. When `false`
///   (the default), any layer containing such features keeps its original
///   geometry bytes intact while structural fixes (extent, version) are still
///   applied.
///
/// ### Example
///
/// ```text
/// from_container filename="bad.versatiles" | vector_repair
/// from_container filename="bad.versatiles" | vector_repair drop_offenders=true
/// ```
pub struct Args {
	/// Drop features that cannot be decoded rather than leaving them in place.
	/// Defaults to false.
	pub drop_offenders: Option<bool>,
}

#[derive(Debug)]
pub struct Operation {
	metadata: TileSourceMetadata,
	source: Arc<Box<dyn TileSource>>,
	tilejson: TileJSON,
	drop_offenders: bool,
}

impl Operation {
	#[context("Building vector_repair operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		Self::new(source, args.drop_offenders.unwrap_or(false))
	}

	pub fn new(source: Box<dyn TileSource>, drop_offenders: bool) -> Result<Operation> {
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
			drop_offenders,
		})
	}
}

/// Decodes `tile`, validates it, and repairs it via [`repair_tile`].
/// Returns the original blob unchanged when the tile is already conformant.
fn do_repair(mut tile: Tile, drop_offenders: bool) -> Result<Tile> {
	let tile_format = tile.format();
	let vt_owned = {
		let vt = tile.as_vector()?;
		if versatiles_geometry::vector_tile::validate_tile(vt).is_empty() {
			return Ok(tile);
		}
		vt.clone()
	};
	let repaired = repair_tile(vt_owned, drop_offenders)?;
	Tile::from_vector(repaired, tile_format)
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
		let drop_offenders = self.drop_offenders;
		Ok(self
			.source
			.tile_stream(bbox)
			.await?
			.map_parallel_try(move |_coord, tile| do_repair(tile, drop_offenders))
			.unwrap_results())
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let Some(tile) = self.source.tile(coord).await? else {
			return Ok(None);
		};
		Ok(Some(do_repair(tile, self.drop_offenders)?))
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
		assert!(
			docs.to_lowercase().contains("repair") || docs.to_lowercase().contains("normalise"),
			"docs should mention repair: {docs}"
		);
	}

	#[tokio::test]
	async fn test_docs_mention_drop_offenders() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(
			docs.contains("drop_offenders"),
			"docs should mention drop_offenders arg: {docs}"
		);
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

	#[tokio::test]
	async fn test_drop_offenders_false_is_default() -> Result<()> {
		let source = Box::new(DummyVectorSource::new(
			&[("dummy", &[&[("k", "v")]])],
			Some(TilePyramid::new_full_up_to(1)),
		));
		let op = Operation::build(
			VPLNode::try_from_str("vector_repair")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;
		assert!(!op.drop_offenders, "drop_offenders should default to false");
		Ok(())
	}

	#[tokio::test]
	async fn test_drop_offenders_true_parsed_from_vpl() -> Result<()> {
		let source = Box::new(DummyVectorSource::new(
			&[("dummy", &[&[("k", "v")]])],
			Some(TilePyramid::new_full_up_to(1)),
		));
		let op = Operation::build(
			VPLNode::try_from_str("vector_repair drop_offenders=true")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;
		assert!(op.drop_offenders);
		Ok(())
	}

	use geo_types::{Geometry, LineString, Polygon};
	use versatiles_core::TileFormat;
	use versatiles_geometry::geo::GeoFeature;
	use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer, validate_tile};

	fn build_clean_tile() -> Tile {
		let outer = LineString::from(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0], [0.0, 0.0]]);
		let poly = Polygon::new(outer, vec![]);
		let feature = GeoFeature::new(Geometry::Polygon(poly));
		let layer = VectorTileLayer::from_features("ok".to_string(), vec![feature], 4096, 1).unwrap();
		Tile::from_vector(VectorTile::new(vec![layer]), TileFormat::MVT).unwrap()
	}

	#[test]
	fn clean_tile_round_trips_without_issues() -> Result<()> {
		let input = build_clean_tile();
		let mut repaired = do_repair(input, false)?;
		let vt = repaired.as_vector()?;
		let issues = validate_tile(vt);
		assert!(
			issues.is_empty(),
			"clean input should remain clean after vector_repair, got {issues:?}",
		);
		assert_eq!(vt.layers.len(), 1);
		assert_eq!(vt.layers[0].features.len(), 1);
		Ok(())
	}

	/// End-to-end: wrap `vector_repair` around `../testdata/berlin.mbtiles`
	/// and verify the validator reports zero issues on the output.
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
		let op = Operation::new(source, false)?;

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
							coord.level,
							coord.x,
							coord.y,
							issues[0].layer,
							issues[0].feature_index.map_or("-".to_string(), |i| i.to_string()),
							issues[0].kind
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
