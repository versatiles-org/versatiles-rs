use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
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
/// Each source is drained into its own `HashSet` concurrently. As each completes,
/// its set is merged into a running accumulator and then dropped, so at most two
/// `HashSet`s are in memory at any time (the accumulator and the one being merged).
pub async fn union_tile_coord_streams(sources: &[&dyn TileSource], bbox: TileBBox) -> Result<TileStream<'static, ()>> {
	let futures: FuturesUnordered<_> = sources
		.iter()
		.map(|s| async {
			let mut stream = s.get_tile_coord_stream(bbox).await?;
			let mut coords = HashSet::new();
			while let Some((coord, ())) = stream.next().await {
				coords.insert(coord);
			}
			Ok::<_, anyhow::Error>(coords)
		})
		.collect();

	let mut union: HashSet<TileCoord> = HashSet::new();
	futures::pin_mut!(futures);
	while let Some(result) = futures.next().await {
		let coords = result?;
		if union.is_empty() {
			union = coords;
		} else {
			union.extend(coords);
		}
	}

	let vec: Vec<(TileCoord, ())> = union.into_iter().map(|c| (c, ())).collect();
	Ok(TileStream::from_vec(vec))
}
