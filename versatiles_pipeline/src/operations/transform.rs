//! The shape of a per-tile transform.
//!
//! Ten of the transform operations are the same thing: a tile goes in, a tile
//! comes out or is dropped, at the same coordinate and over the same pyramid.
//! Written out by hand that is six `TileSource` methods each, four of them pure
//! delegation, and the interesting part is one closure in the middle.
//!
//! [`TileTransform`] is that closure, and [`TransformOp`] is the six methods,
//! written once. The nine transforms that genuinely restructure — building
//! parents, reading a different zoom, changing tile size, restricting the
//! pyramid, relabelling coordinates — keep their own implementations, because
//! their `tile_stream` differs for reasons that matter.

use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TileStreamErrorExt, TilesRuntime};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

/// One tile in, one tile out or dropped.
///
/// Implementors hold the operation's parameters and nothing else: the source,
/// the metadata, the runtime and the streaming all live in [`TransformOp`].
pub trait TileTransform: Debug + Send + Sync + 'static {
	/// The operation's VPL tag, e.g. `"raster_flatten"`.
	///
	/// One home for it. `source_type` reports it, per-tile errors are recorded
	/// under it, and the trace line names it — three places that used to be
	/// three string literals per file, free to drift apart.
	const TAG: &'static str;

	/// Adjusts the metadata the operation reports, for the operations that
	/// change it. `raster_format` changes the tile format; most change nothing.
	fn update_metadata(&self, _metadata: &mut TileSourceMetadata) {}

	/// Adjusts the `TileJSON` the operation reports.
	fn update_tilejson(&self, _tilejson: &mut TileJSON) {}

	/// Transforms one tile, or returns `Ok(None)` to drop it.
	///
	/// An `Err` is recorded against the runtime and the tile is dropped; see
	/// [`TileStreamErrorExt::record_errors`]. `coord` is passed because some
	/// operations need it (`dem_quantize` derives its quantizer from the zoom
	/// level); operations that do not can ignore it.
	///
	/// Called once per tile, so anything expensive that depends only on the
	/// operation's parameters belongs in the implementor's fields, computed at
	/// build time.
	fn run(&self, coord: &TileCoord, tile: Tile) -> Result<Option<Tile>>;
}

/// The typed convenience for vector operations: [`TileTransform`] with the
/// decoding and re-encoding already done.
///
/// Not a way to avoid `TileTransform` but a way to state that an operation
/// works on features rather than bytes — `vector_filter_layers` has no reason
/// to know that a `Tile` exists. Wrap one in [`AsTileTransform`] to use it.
pub trait VectorTransform: Debug + Send + Sync + 'static {
	/// The operation's VPL tag. See [`TileTransform::TAG`].
	const TAG: &'static str;

	/// Adjusts the `TileJSON` the operation reports.
	fn update_tilejson(&self, _tilejson: &mut TileJSON) {}

	/// Transforms one decoded tile, or returns `Ok(None)` to drop it.
	fn run(&self, tile: VectorTile) -> Result<Option<VectorTile>>;
}

/// Adapts a [`VectorTransform`] into a [`TileTransform`].
#[derive(Debug)]
pub struct AsTileTransform<V: VectorTransform>(pub V);

impl<V: VectorTransform> TileTransform for AsTileTransform<V> {
	const TAG: &'static str = V::TAG;

	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		self.0.update_tilejson(tilejson);
	}

	fn run(&self, _coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		let format = tile.format();
		let Some(vector) = self.0.run(tile.into_vector()?)? else {
			return Ok(None);
		};
		Ok(Some(Tile::from_vector(vector, format)?))
	}
}

/// A [`TileSource`] that applies a [`TileTransform`] to everything its source
/// produces.
#[derive(Debug)]
pub struct TransformOp<T: TileTransform> {
	transform: Arc<T>,
	source: Box<dyn TileSource>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	runtime: TilesRuntime,
}

impl<T: TileTransform> TransformOp<T> {
	/// Wraps `source`, applying `transform` to each tile.
	///
	/// The metadata and `TileJSON` are cloned from the source and then handed to
	/// the transform to adjust.
	pub fn new(source: Box<dyn TileSource>, transform: T, runtime: TilesRuntime) -> Self {
		let mut metadata = source.metadata().clone();
		transform.update_metadata(&mut metadata);

		let mut tilejson = source.tilejson().clone();
		transform.update_tilejson(&mut tilejson);
		metadata.update_tilejson(&mut tilejson);

		TransformOp {
			transform: Arc::new(transform),
			source,
			metadata,
			tilejson,
			runtime,
		}
	}

	/// The transform itself, so an operation's tests can check what its
	/// arguments parsed to without going through a tile.
	#[cfg(test)]
	pub fn transform(&self) -> &T {
		&self.transform
	}
}

#[async_trait]
impl<T: TileTransform> TileSource for TransformOp<T> {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor(T::TAG, self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// The transform's pyramid when it declared one — `raster_mask` clips the
	/// pyramid to its mask so tiles outside it are never requested — and the
	/// source's otherwise.
	///
	/// Not simply the cloned metadata: a source that computes its pyramid lazily
	/// has nothing there to read until asked.
	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		match self.metadata.tile_pyramid() {
			Some(pyramid) => Ok(pyramid),
			None => self.source.tile_pyramid().await,
		}
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("{}::tile_stream {bbox:?}", T::TAG);

		let transform = Arc::clone(&self.transform);
		Ok(self
			.source
			.tile_stream(bbox)
			.await?
			.filter_map_parallel_try(move |coord, tile| transform.run(&coord, tile))
			.record_errors(&self.runtime, T::TAG))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

#[cfg(test)]
mod tests {
	use anyhow::anyhow;
	use versatiles_core::TilePyramid;

	use super::*;
	use crate::helpers::dummy_vector_source::DummyVectorSource;

	#[derive(Debug)]
	struct Passthrough;

	impl TileTransform for Passthrough {
		const TAG: &'static str = "passthrough";
		fn run(&self, _coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
			Ok(Some(tile))
		}
	}

	#[derive(Debug)]
	struct DropEverything;

	impl TileTransform for DropEverything {
		const TAG: &'static str = "drop_everything";
		fn run(&self, _coord: &TileCoord, _tile: Tile) -> Result<Option<Tile>> {
			Ok(None)
		}
	}

	#[derive(Debug)]
	struct Fail;

	impl TileTransform for Fail {
		const TAG: &'static str = "fail";
		fn run(&self, _coord: &TileCoord, _tile: Tile) -> Result<Option<Tile>> {
			Err(anyhow!("this tile is beyond help"))
		}
	}

	fn source() -> Box<dyn TileSource> {
		Box::new(DummyVectorSource::new(
			&[("layer1", &[&[("key", "val")]])],
			Some(TilePyramid::new_full_up_to(2)),
		))
	}

	fn op<T: TileTransform>(transform: T) -> TransformOp<T> {
		TransformOp::new(source(), transform, TilesRuntime::new_silent())
	}

	#[tokio::test]
	async fn tiles_pass_through() -> Result<()> {
		let tiles = op(Passthrough)
			.tile_stream(TileBBox::new_full(1)?)
			.await?
			.to_vec()
			.await;
		assert_eq!(tiles.len(), 4);
		Ok(())
	}

	#[tokio::test]
	async fn a_transform_can_drop_a_tile() -> Result<()> {
		let tiles = op(DropEverything)
			.tile_stream(TileBBox::new_full(1)?)
			.await?
			.to_vec()
			.await;
		assert!(tiles.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn a_failing_tile_is_recorded_and_dropped() -> Result<()> {
		let runtime = TilesRuntime::new_silent();
		let operation = TransformOp::new(source(), Fail, runtime.clone());

		let tiles = operation.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;

		assert!(tiles.is_empty(), "no tile survives");
		assert_eq!(
			runtime.error_count(),
			4,
			"and each failure is recorded, not panicked on"
		);
		Ok(())
	}

	#[tokio::test]
	async fn the_operation_reports_its_own_tag() {
		assert!(op(Passthrough).source_type().to_string().contains("passthrough"));
		assert!(op(DropEverything).source_type().to_string().contains("drop_everything"));
	}

	#[tokio::test]
	async fn coordinates_and_pyramid_come_from_the_source() -> Result<()> {
		let operation = op(Passthrough);
		let bbox = TileBBox::new_full(1)?;
		assert_eq!(operation.tile_coord_stream(bbox).await?.to_vec().await.len(), 4);
		assert!(!operation.tile_pyramid().await?.is_empty());
		Ok(())
	}
}
