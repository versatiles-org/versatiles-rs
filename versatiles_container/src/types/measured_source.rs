//! A [`TileSource`] that counts its work for a [`Stage`]: the CPU time of
//! everything it does, the tiles it delivers, and how long whatever reads from
//! it waits for them.
//!
//! A pipeline wraps each operation in one while `--stats` is on, which is how a
//! run can tell which operation holds the others up.

use std::{
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
	time::Instant,
};

use anyhow::Result;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{Stage, TileBBox, TileCoord, TileJSON, TilePyramid, TileSize, TileStream};

use crate::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime};

/// Wraps a source and counts its work for a [`Stage`]: the CPU time of
/// everything it does, the tiles it delivers, and how long whatever reads from
/// it waits for them.
#[derive(Debug)]
pub struct MeasuredSource {
	source: Box<dyn TileSource>,
	stage: Arc<Stage>,
}

impl MeasuredSource {
	/// Counts the work of `source` for `stage`.
	#[must_use]
	pub fn new(source: Box<dyn TileSource>, stage: Arc<Stage>) -> Self {
		Self { source, stage }
	}

	/// `source` wrapped for a new stage named `name`, or `source` itself while
	/// `runtime` measures no stages.
	#[must_use]
	pub fn wrap(
		source: Box<dyn TileSource>,
		name: &str,
		input: Option<Arc<Stage>>,
		runtime: &TilesRuntime,
	) -> Box<dyn TileSource> {
		match runtime.stage(name, input) {
			Some(stage) => Box::new(Self::new(source, stage)),
			None => source,
		}
	}

	/// The stage the work is counted for.
	#[must_use]
	pub fn stage(&self) -> &Arc<Stage> {
		&self.stage
	}
}

/// Every method is forwarded explicitly, as for [`SharedTileSource`], so that
/// no override of the wrapped source is bypassed.
#[async_trait]
impl TileSource for MeasuredSource {
	fn source_type(&self) -> Arc<SourceType> {
		self.source.source_type()
	}

	fn metadata(&self) -> &TileSourceMetadata {
		self.source.metadata()
	}

	fn tilejson(&self) -> &TileJSON {
		self.source.tilejson()
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.stage.instrument(self.source.tile_pyramid()).await
	}

	async fn measure_tile_size(&self) -> Result<Option<TileSize>> {
		self.stage.instrument(self.source.measure_tile_size()).await
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let asked = Instant::now();
		let tile = self.stage.instrument(self.source.tile(coord)).await;
		self.stage.add_wait(asked.elapsed());
		if matches!(tile, Ok(Some(_))) {
			self.stage.add_tile();
		}
		tile
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let asked = Instant::now();
		let stream = self.stage.instrument(self.source.tile_stream(bbox)).await;
		self.stage.add_wait(asked.elapsed());
		Ok(TileStream::from_stream(Box::pin(MeasuredStream {
			inner: stream?.inner,
			stage: Arc::clone(&self.stage),
			pending_since: None,
		})))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.stage.instrument(self.source.tile_coord_stream(bbox)).await
	}

	async fn tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		self.stage.instrument(self.source.tile_size_stream(bbox)).await
	}

	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &mut PrettyPrint, runtime: &TilesRuntime) -> Result<()> {
		self.source.probe_container(print, runtime).await
	}

	fn into_shared(self) -> SharedTileSource {
		Arc::new(self)
	}
}

/// A tile stream whose polls are counted for a stage, and whose consumer's
/// waiting is: from the first poll that finds no tile ready to the poll that
/// gets one.
struct MeasuredStream<T> {
	inner: futures::stream::BoxStream<'static, (TileCoord, T)>,
	stage: Arc<Stage>,
	pending_since: Option<Instant>,
}

impl<T> Stream for MeasuredStream<T> {
	type Item = (TileCoord, T);

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let Self {
			inner,
			stage,
			pending_since,
		} = self.get_mut();
		let poll = stage.in_poll(|| inner.poll_next_unpin(cx));
		match &poll {
			Poll::Pending => {
				pending_since.get_or_insert_with(Instant::now);
			}
			Poll::Ready(item) => {
				if let Some(since) = pending_since.take() {
					stage.add_wait(since.elapsed());
				}
				if item.is_some() {
					stage.add_tile();
				}
			}
		}
		poll
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockReader, MockReaderProfile};

	#[tokio::test]
	async fn tiles_and_waiting_are_counted() -> Result<()> {
		let runtime = TilesRuntime::default();
		runtime.measure_stages();
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?.boxed();
		let source = MeasuredSource::wrap(reader, "mock", None, &runtime);
		let bbox = TileBBox::from_min_and_max(3, 0, 0, 7, 7)?;
		let tiles = source.tile_stream(bbox).await?.to_vec().await;
		let (coord, _) = tiles.first().expect("the mock has tiles at level 3");
		assert!(source.tile(coord).await?.is_some());
		let count = u64::try_from(tiles.len())?;

		let stats = runtime.stage_stats();
		assert_eq!(stats.len(), 1);
		assert_eq!(stats[0].name, "mock");
		assert_eq!(stats[0].tiles, count + 1);
		assert!(stats[0].waited > std::time::Duration::ZERO);
		Ok(())
	}

	#[tokio::test]
	async fn nothing_is_wrapped_while_stages_are_not_measured() -> Result<()> {
		let runtime = TilesRuntime::default();
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?.boxed();
		let source = MeasuredSource::wrap(reader, "mock", None, &runtime);
		assert_eq!(runtime.stage_stats(), Vec::new());
		assert!(format!("{source:?}").starts_with("MockReader"), "{source:?}");
		Ok(())
	}
}
