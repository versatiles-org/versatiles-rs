use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use futures::StreamExt;
use std::collections::HashSet;
use versatiles_container::TileSource;
use versatiles_core::{TileBBox, TileCoord, TileStream};

pub trait ReadTileSource: TileSource {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource;
}

/// Collect the union of tile coordinates from multiple sources into a single stream.
///
/// All sources are queried concurrently for their tile coordinates within `bbox`.
/// The results are deduplicated so that each coordinate appears at most once.
pub async fn union_tile_coord_streams(sources: &[&dyn TileSource], bbox: TileBBox) -> Result<TileStream<'static, ()>> {
	let streams: Vec<TileStream<'_, ()>> =
		futures::future::try_join_all(sources.iter().map(|s| s.get_tile_coord_stream(bbox))).await?;

	let mut coords = HashSet::new();
	let mut merged = futures::stream::select_all(streams.into_iter().map(|s| s.inner));
	while let Some((coord, _)) = merged.next().await {
		coords.insert(coord);
	}

	let vec: Vec<(TileCoord, ())> = coords.into_iter().map(|c| (c, ())).collect();
	Ok(TileStream::from_vec(vec))
}
