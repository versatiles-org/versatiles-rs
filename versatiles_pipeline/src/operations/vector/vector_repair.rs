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

use anyhow::{Result, ensure};
use versatiles_container::{Tile, TileSource, TilesRuntime};
use versatiles_core::{TileCoord, TileType};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::repair_tile;

use crate::{
	PipelineFactory,
	operations::transform::{TileTransform, TransformOp},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Repairs vector tiles so that they conform to MVT 2.1.
///
/// Always fixed: missing `extent` and `version` fields, duplicate layer names,
/// inverted polygon winding, and degenerate rings.
///
/// Tiles the validator considers clean pass through unchanged — the original
/// encoded blob is forwarded without re-encoding — so this operation is cheap
/// on conformant input.
///
/// Without `drop_offenders`, a layer holding a feature whose geometry cannot be
/// decoded keeps its original geometry bytes, while the structural fixes are
/// still applied to it.
///
/// ### Example
///
/// ```vpl
/// from_container filename="bad.versatiles" | vector_repair
/// from_container filename="bad.versatiles" | vector_repair drop_offenders=true
/// ```
pub struct Args {
	/// Whether to remove features whose geometry cannot be decoded. Defaults to `false`.
	pub drop_offenders: Option<bool>,
}

#[derive(Debug)]
pub struct Operation {
	drop_offenders: bool,
}

impl Operation {
	#[context("Building vector_repair operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		Self::new(source, args.drop_offenders.unwrap_or(false), factory.runtime())
	}

	pub fn new(
		source: Box<dyn TileSource>,
		drop_offenders: bool,
		runtime: TilesRuntime,
	) -> Result<TransformOp<Operation>> {
		ensure!(
			source.metadata().tile_format().to_type() == TileType::Vector,
			"vector_repair requires a vector tile source"
		);
		Ok(TransformOp::new(source, Operation { drop_offenders }, runtime))
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "vector_repair";

	fn run(&self, _coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		do_repair(tile, self.drop_offenders).map(Some)
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

crate::operations::macros::define_transform_factory!("vector_repair", Args, Operation, requires: Vector);

#[cfg(test)]
mod tests {
	use versatiles_container::TileSource;
	use versatiles_core::TilePyramid;

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_vector_source::DummyVectorSource};

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
		use versatiles_core::TileFormat;

		use crate::helpers::dummy_image_source::DummyImageSource;
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
		assert!(!op.transform().drop_offenders, "drop_offenders should default to false");
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
		assert!(op.transform().drop_offenders);
		Ok(())
	}

	use geo_types::{Geometry, LineString, Polygon};
	use versatiles_core::TileFormat;
	use versatiles_geometry::{
		geo::GeoFeature,
		vector_tile::{VectorTile, VectorTileLayer, validate_tile},
	};

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
		let op = Operation::new(source, false, TilesRuntime::new_silent())?;

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
