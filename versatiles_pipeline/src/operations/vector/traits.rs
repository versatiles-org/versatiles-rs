use std::sync::Arc;

use anyhow::{Result, ensure};
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TileStreamErrorExt, TilesRuntime};
use versatiles_core::{TileBBox, TileJSON, TilePyramid, TileStream, TileType};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

pub trait RunnerTrait: std::fmt::Debug + Send + Sync + 'static {
	fn update_tilejson(&self, tilejson: &mut TileJSON);
	fn run(&self, tile: VectorTile) -> Result<Option<VectorTile>>;
}

/// Generic “transform” operation that delegates all real work to a `Runner`.
#[derive(Debug)]
pub struct TransformOp<R: RunnerTrait> {
	pub runner: Arc<R>,
	pub runtime: TilesRuntime,
	/// The operation's own VPL tag. One home for it: `source_type` reports it,
	/// and a per-tile error is recorded under it.
	pub tag: &'static str,
	pub source: Box<dyn TileSource>,
	pub metadata: TileSourceMetadata,
	pub tilejson: TileJSON,
}

#[async_trait]
impl<R: RunnerTrait> TileSource for TransformOp<R> {
	/* --- metadata --- */
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor(self.tag, self.source.source_type())
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.source.tile_pyramid().await
	}

	#[context("Failed to get transformed tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("{}::tile_stream {bbox:?}", self.tag);
		let runner = self.runner.clone();
		let tile_format = *self.metadata.tile_format();
		Ok(self
			.source
			.tile_stream(bbox)
			.await?
			.filter_map_parallel_try(move |_coord, tile| {
				let vector = tile.into_vector()?;
				if let Some(transformed_vector) = runner.run(vector)? {
					Ok(Some(Tile::from_vector(transformed_vector, tile_format)?))
				} else {
					Ok(None)
				}
			})
			.record_errors(&self.runtime, self.tag))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

// transform_factory.rs
#[context("Failed to build transform operation")]
pub async fn build_transform<R>(
	source: Box<dyn TileSource>,
	runner: R,
	tag: &'static str,
	runtime: TilesRuntime,
) -> Result<Box<dyn TileSource>>
where
	R: RunnerTrait,
{
	// ── common steps ───────────────────────────────────────────────
	let metadata = source.metadata().clone();
	ensure!(
		metadata.tile_format().to_type() == TileType::Vector,
		"source must be vector tiles"
	);

	// ── runner creation delegated to the caller ────────────────────
	let runner = Arc::new(runner);

	// ── tile-json patching (always the same) ───────────────────────
	let mut tilejson = source.tilejson().clone();
	runner.update_tilejson(&mut tilejson);
	metadata.update_tilejson(&mut tilejson);

	Ok(Box::new(TransformOp::<R> {
		runner,
		runtime,
		tag,
		source,
		metadata,
		tilejson,
	}) as Box<dyn TileSource>)
}

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use versatiles_core::{TileBBox, TileCoord, TilePyramid};
	use versatiles_geometry::vector_tile::VectorTile;

	use super::*;
	use crate::helpers::dummy_vector_source::DummyVectorSource;

	#[derive(Debug)]
	struct PassthroughRunner;

	impl RunnerTrait for PassthroughRunner {
		fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
		fn run(&self, tile: VectorTile) -> Result<Option<VectorTile>> {
			Ok(Some(tile))
		}
	}

	#[derive(Debug)]
	struct DropRunner;

	impl RunnerTrait for DropRunner {
		fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
		fn run(&self, _tile: VectorTile) -> Result<Option<VectorTile>> {
			Ok(None)
		}
	}

	fn make_vector_source() -> DummyVectorSource {
		DummyVectorSource::new(
			&[("layer1", &[&[("key", "val")]])],
			Some(TilePyramid::new_full_up_to(2)),
		)
	}

	#[tokio::test]
	async fn test_build_transform_passthrough() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner, "test_transform", TilesRuntime::new_silent()).await?;
		let bbox = TileBBox::new_full(0)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert!(!tiles.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_build_transform_drops_tiles() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, DropRunner, "test_transform", TilesRuntime::new_silent()).await?;
		let bbox = TileBBox::new_full(0)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert!(tiles.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_build_transform_rejects_raster_source() {
		use versatiles_core::TileFormat;

		use crate::helpers::dummy_image_source::DummyImageSource;
		let source = Box::new(DummyImageSource::from_color(&[128u8, 0, 0], 256, TileFormat::PNG, None).unwrap());
		let result = build_transform(source, PassthroughRunner, "test_transform", TilesRuntime::new_silent()).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_transform_op_metadata_and_tilejson() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner, "test_transform", TilesRuntime::new_silent()).await?;
		assert!(!op.metadata().tile_pyramid().unwrap().is_empty());
		assert!(op.tilejson().zoom_min().is_some());
		Ok(())
	}

	#[tokio::test]
	async fn test_transform_op_tile_coord_stream() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner, "test_transform", TilesRuntime::new_silent()).await?;
		let bbox = TileBBox::new_full(0)?;
		let coords = op.tile_coord_stream(bbox).await?.to_vec().await;
		assert!(!coords.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_transform_op_source_type() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner, "test_transform", TilesRuntime::new_silent()).await?;
		assert!(
			op.source_type().to_string().contains("test_transform"),
			"the operation reports its own tag, not a shared one"
		);
		Ok(())
	}

	#[derive(Debug)]
	struct FailingRunner;

	impl RunnerTrait for FailingRunner {
		fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
		fn run(&self, _tile: VectorTile) -> Result<Option<VectorTile>> {
			Err(anyhow::anyhow!("this tile is beyond help"))
		}
	}

	#[tokio::test]
	async fn a_failing_tile_is_recorded_rather_than_panicking() -> Result<()> {
		// This used to be `unwrap_results()`, i.e. one bad tile took the whole
		// process down and no caller could decide otherwise.
		let runtime = TilesRuntime::new_silent();
		let source = Box::new(make_vector_source());
		let op = build_transform(source, FailingRunner, "test_transform", runtime.clone()).await?;

		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;

		assert!(tiles.is_empty(), "every tile failed, so none come through");
		assert_eq!(runtime.error_count(), 4, "and every failure was recorded");
		Ok(())
	}

	#[tokio::test]
	async fn a_failing_tile_reaches_tile_the_same_way() -> Result<()> {
		// `tile()` routes through `tile_stream`, so it inherits the policy rather
		// than needing an override of its own — which is what vector_repair had.
		let runtime = TilesRuntime::new_silent();
		let source = Box::new(make_vector_source());
		let op = build_transform(source, FailingRunner, "test_transform", runtime.clone()).await?;

		assert!(op.tile(&TileCoord::new(1, 0, 0)?).await?.is_none());
		assert_eq!(runtime.error_count(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn a_conversion_fails_instead_of_taking_the_process_with_it() -> Result<()> {
		// The end of the chain the policy exists for: every tile fails, each one is
		// recorded, and because this runtime was asked to abort, the conversion
		// reports it rather than writing a quietly truncated archive. Before R5
		// this call panicked.
		use std::sync::Arc;

		use versatiles_container::{TilesConverterParameters, convert_tiles_container_to_str};

		let runtime = TilesRuntime::new_silent();
		runtime.set_abort_on_error(true);

		let source = Box::new(make_vector_source());
		let op = build_transform(source, FailingRunner, "test_transform", runtime.clone()).await?;

		let dir = tempfile::tempdir()?;
		let path = dir.path().join("out.versatiles");
		let error = convert_tiles_container_to_str(
			Arc::new(op),
			TilesConverterParameters::default(),
			path.to_str().unwrap(),
			runtime.clone(),
		)
		.await
		.expect_err("a conversion in which every tile failed should not report success");

		let message = format!("{error:#}");
		assert!(
			message.contains("output may be incomplete"),
			"should say the archive cannot be trusted: {message}"
		);
		assert!(runtime.error_count() > 0, "and each failed tile was counted");
		Ok(())
	}
}
