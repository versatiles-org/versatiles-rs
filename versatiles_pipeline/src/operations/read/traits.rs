use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
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
/// Each source is queried for its tile coordinates within `bbox`. The results are
/// deduplicated so that each coordinate appears at most once.
pub async fn union_tile_coord_streams(sources: &[&dyn TileSource], bbox: TileBBox) -> Result<TileStream<'static, ()>> {
	let mut coords = HashSet::new();
	for source in sources {
		let mut stream = source.get_tile_coord_stream(bbox).await?;
		while let Some((coord, _)) = stream.next().await {
			coords.insert(coord);
		}
	}
	let vec: Vec<(TileCoord, ())> = coords.into_iter().map(|c| (c, ())).collect();
	Ok(TileStream::from_vec(vec))
}
