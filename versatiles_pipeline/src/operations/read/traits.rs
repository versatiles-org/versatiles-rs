use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use std::collections::HashSet;
use versatiles_container::TileSource;
use versatiles_core::{TileBBox, TileCoord, TileStream};

/// Marker trait implemented by each read operation's `Operation` type to
/// host its `build` factory. The build result is `Box<dyn TileSource>`, which
/// can be a *different* type than `Self` — useful when the actual runtime
/// `TileSource` is shared across formats (see
/// [`crate::helpers::feature_tile_source::FeatureTileSource`]).
pub trait ReadTileSource {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized;
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
			let mut stream = s.tile_coord_stream(bbox).await?;
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::dummy_vector_source::DummyVectorSource;
	use versatiles_core::TilePyramid;

	#[tokio::test]
	async fn test_union_empty_sources() -> Result<()> {
		let bbox = TileBBox::new_full(2)?;
		let mut stream = union_tile_coord_streams(&[], bbox).await?;
		assert!(stream.next().await.is_none());
		Ok(())
	}

	#[tokio::test]
	async fn test_union_single_source() -> Result<()> {
		let pyramid = TilePyramid::new_full_up_to(2);
		let source = DummyVectorSource::new(&[("layer", &[])], Some(pyramid));
		let bbox = TileBBox::new_full(1)?;
		let sources: &[&dyn TileSource] = &[&source];
		let stream = union_tile_coord_streams(sources, bbox).await?;
		let coords = stream.to_vec().await;
		// level 1 full = 4 tiles
		assert_eq!(coords.len(), 4);
		Ok(())
	}

	#[tokio::test]
	async fn test_union_two_non_overlapping_sources() -> Result<()> {
		use versatiles_core::GeoBBox;
		let pyramid1 = TilePyramid::from_geo_bbox(1, 1, &GeoBBox::new(-180.0, -85.0, 0.0, 85.0).unwrap()).unwrap();
		let pyramid2 = TilePyramid::from_geo_bbox(1, 1, &GeoBBox::new(0.0, -85.0, 180.0, 85.0).unwrap()).unwrap();
		let source1 = DummyVectorSource::new(&[("layer", &[])], Some(pyramid1));
		let source2 = DummyVectorSource::new(&[("layer", &[])], Some(pyramid2));
		let bbox = TileBBox::new_full(1)?;
		let sources: &[&dyn TileSource] = &[&source1, &source2];
		let stream = union_tile_coord_streams(sources, bbox).await?;
		let coords = stream.to_vec().await;
		assert_eq!(coords.len(), 4);
		Ok(())
	}

	#[tokio::test]
	async fn test_union_overlapping_sources_deduplicates() -> Result<()> {
		let pyramid = TilePyramid::new_full_up_to(1);
		let source1 = DummyVectorSource::new(&[("layer", &[])], Some(pyramid.clone()));
		let source2 = DummyVectorSource::new(&[("layer", &[])], Some(pyramid));
		let bbox = TileBBox::new_full(1)?;
		let sources: &[&dyn TileSource] = &[&source1, &source2];
		let stream = union_tile_coord_streams(sources, bbox).await?;
		let coords = stream.to_vec().await;
		// Both sources have the same 4 tiles; union should deduplicate
		assert_eq!(coords.len(), 4);
		Ok(())
	}
}
