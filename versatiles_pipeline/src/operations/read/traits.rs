use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use std::collections::HashSet;
use versatiles_container::TileSource;
use versatiles_core::{ConcurrencyLimits, TileBBox, TileCoord, TileStream};

// ---------------------------------------------------------------------------
// Bounded gathering: shared memory budget for operations that read tiles from
// several sources / coordinates at once (`from_merged_vector`, `from_stacked`,
// `from_stacked_raster`). Without a cap, peak memory grows with concurrency ×
// tile size, which OOMs on large tiles, many sources, or huge bboxes.
// ---------------------------------------------------------------------------

/// Number of read chunks processed concurrently while gathering source tiles:
/// one chunk is processed while the next is read. Kept small so the tile budget
/// translates into a tight memory bound.
pub const READ_AHEAD: usize = 2;

/// Default cap on the number of raw source tiles a gathering operation keeps in
/// memory at once. Peak memory ≈ this × the largest tile size, so lower it for
/// very large tiles or many sources via `VERSATILES_MAX_TILES_IN_FLIGHT`.
const DEFAULT_MAX_TILES_IN_FLIGHT: usize = 2048;

/// The configured cap on resident raw source tiles (env-overridable via
/// `VERSATILES_MAX_TILES_IN_FLIGHT`).
#[must_use]
pub fn max_tiles_in_flight() -> usize {
	std::env::var("VERSATILES_MAX_TILES_IN_FLIGHT")
		.ok()
		.and_then(|s| s.trim().parse::<usize>().ok())
		.filter(|&n| n > 0)
		.unwrap_or(DEFAULT_MAX_TILES_IN_FLIGHT)
}

/// Largest power-of-two grid cell size (tiles per side) for which `READ_AHEAD`
/// chunks — each holding `tiles_per_coord` tiles per coordinate — stay within the
/// configured budget.
///
/// Guarantees resident raw tiles ≤ `READ_AHEAD × size² × tiles_per_coord ≤
/// max_tiles_in_flight()`, independent of the requested bbox size.
#[must_use]
pub fn chunk_grid_size(tiles_per_coord: usize) -> u32 {
	let budget =
		u64::try_from((max_tiles_in_flight() / (READ_AHEAD * tiles_per_coord.max(1))).max(1)).unwrap_or(u64::MAX);
	let mut size: u64 = 1;
	// Cap at 4096 (a 4096² cell is already 16M tiles) to keep `size` within u32.
	while size < 4096 && (size * 2) * (size * 2) <= budget {
		size *= 2;
	}
	u32::try_from(size).unwrap_or(4096)
}

/// Per-coordinate concurrency for per-coordinate gathering paths (e.g. raster
/// blending), bounded by both the tile budget and the CPU core count.
///
/// Ensures resident tiles ≤ `concurrency × tiles_per_coord ≤ max_tiles_in_flight()`
/// while never exceeding the CPU-bound limit.
#[must_use]
pub fn coord_concurrency(tiles_per_coord: usize) -> usize {
	let by_budget = (max_tiles_in_flight() / tiles_per_coord.max(1)).max(1);
	by_budget.min(ConcurrencyLimits::default().cpu_bound)
}

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

	#[test]
	fn test_chunk_grid_size_is_power_of_two() {
		for tiles_per_coord in 1..=8 {
			assert!(chunk_grid_size(tiles_per_coord).is_power_of_two());
		}
	}

	#[test]
	fn test_chunk_grid_size_respects_budget() {
		// READ_AHEAD × size² × tiles_per_coord must not exceed the budget
		// (unless clamped to the minimum cell of 1×1).
		let max = max_tiles_in_flight();
		for tiles_per_coord in [1usize, 2, 3, 8, 64] {
			let g = u64::from(chunk_grid_size(tiles_per_coord));
			let resident = READ_AHEAD as u64 * g * g * tiles_per_coord as u64;
			assert!(
				resident <= max as u64 || g == 1,
				"resident {resident} exceeds budget {max} (tiles_per_coord {tiles_per_coord})"
			);
		}
	}

	#[test]
	fn test_coord_concurrency_is_bounded() {
		let cpu = ConcurrencyLimits::default().cpu_bound;
		let max = max_tiles_in_flight();
		for tiles_per_coord in [1usize, 2, 8, 100] {
			let c = coord_concurrency(tiles_per_coord);
			assert!(c >= 1);
			assert!(c <= cpu, "must not exceed CPU-bound limit");
			assert!(c * tiles_per_coord <= max || c == 1, "must stay within tile budget");
		}
	}
}
