//! Flat-map transformation methods for TileStream.
//!
//! This module provides `flat_map` methods that transform each tile into multiple tiles:
//!
//! | Method | Callback | Execution |
//! |--------|----------|-----------|
//! | `flat_map_parallel_try` | `Fn(TileCoord, T) -> Result<TileStream<O>>` | sync, parallel |

use super::{Arc, ConcurrencyLimits, Result, StreamExt, TileCoord, TileStream, ready, stream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Sync Parallel
	// -------------------------------------------------------------------------

	/// Transforms each tile into multiple tiles using parallel processing.
	///
	/// Each `(coord, value)` pair is mapped to a stream of tiles via the `callback` function.
	/// The callback runs in a blocking task pool (CPU-bound concurrency limit) and all
	/// resulting streams are flattened into a single output stream.
	///
	/// Returns a stream of `Result<O>` values. If the callback returns an error for any tile,
	/// that error is propagated as an item in the stream. Successful callbacks produce
	/// sub-streams whose items are wrapped in `Ok`.
	///
	/// # Use Cases
	/// - Tile subdivision: split one tile into four child tiles at a higher zoom level
	/// - Multi-resolution generation: create multiple zoom levels from source tiles
	/// - Format conversion with variants: generate both compressed and uncompressed versions
	///
	/// # Concurrency
	/// Uses CPU-bound concurrency limit since callbacks run in `spawn_blocking`.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<TileStream<O>>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # use futures::{StreamExt, TryStreamExt};
	/// # async fn example() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(1, 0, 0)?, Blob::from("tile")),
	/// ]);
	///
	/// // Subdivide each tile into 4 child tiles
	/// let subdivided = stream.flat_map_parallel_try(|coord, blob| {
	///     let child_level = coord.level + 1;
	///     let children = vec![
	///         (TileCoord::new(child_level, coord.x * 2, coord.y * 2)?, blob.clone()),
	///         (TileCoord::new(child_level, coord.x * 2 + 1, coord.y * 2)?, blob.clone()),
	///         (TileCoord::new(child_level, coord.x * 2, coord.y * 2 + 1)?, blob.clone()),
	///         (TileCoord::new(child_level, coord.x * 2 + 1, coord.y * 2 + 1)?, blob),
	///     ];
	///     Ok(TileStream::from_vec(children))
	/// });
	///
	/// // Collect results, failing fast on first error
	/// let tiles: Vec<(TileCoord, Blob)> = subdivided
	///     .inner
	///     .filter_map(|(coord, result)| async move {
	///         match result {
	///             Ok(item) => Some(Ok((coord, item))),
	///             Err(e) => Some(Err(e)),
	///         }
	///     })
	///     .try_collect()
	///     .await?;
	/// assert_eq!(tiles.len(), 4); // 1 input tile → 4 output tiles
	/// # Ok(())
	/// # }
	/// ```
	pub fn flat_map_parallel_try<F, O>(self, callback: F) -> TileStream<'a, Result<O>>
	where
		F: Fn(TileCoord, T) -> Result<TileStream<'static, O>> + Send + Sync + 'static,
		T: 'static,
		O: Send + 'static,
		'a: 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(coord, item)))
			})
			.buffer_unordered(limits.cpu_bound) // CPU-bound: spawn_blocking
			.flat_map_unordered(None, |result| match result {
				Ok((_coord, Ok(sub_stream))) => {
					// Successful callback: wrap each item from the sub-stream in Ok
					sub_stream.inner.map(|(c, item)| (c, Ok(item))).boxed()
				}
				Ok((coord, Err(e))) => {
					// Failed callback: emit a single error
					stream::once(ready((
						coord,
						Err(e.context(format!("Failed to process tile at {coord:?}"))),
					)))
					.boxed()
				}
				Err(e) => panic!("Spawned task panicked: {e}"), // Task panic is still a panic (unexpected)
			});
		TileStream { inner: s.boxed() }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Blob;
	use anyhow::bail;

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	// -------------------------------------------------------------------------
	// flat_map_parallel_try (sync, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_flat_map_parallel_try_subdivide() {
		let stream = TileStream::from_vec(vec![(tc(1, 0, 0), Blob::from("tile"))]);

		let subdivided = stream.flat_map_parallel_try(|coord, blob| {
			let child_level = coord.level + 1;
			let children = vec![
				(tc(child_level, coord.x * 2, coord.y * 2), blob.clone()),
				(tc(child_level, coord.x * 2 + 1, coord.y * 2), blob.clone()),
				(tc(child_level, coord.x * 2, coord.y * 2 + 1), blob.clone()),
				(tc(child_level, coord.x * 2 + 1, coord.y * 2 + 1), blob),
			];
			Ok(TileStream::from_vec(children))
		});

		let tiles: Vec<(TileCoord, Blob)> = subdivided.unwrap_results().to_vec().await;
		assert_eq!(tiles.len(), 4);
	}

	#[tokio::test]
	async fn test_flat_map_parallel_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let flat = stream.flat_map_parallel_try(|_coord, _blob| bail!("test error"));

		let items: Vec<(TileCoord, Result<Blob>)> = flat.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}

	#[tokio::test]
	async fn test_flat_map_parallel_try_empty_result() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let flat = stream.flat_map_parallel_try(|_coord, _blob| Ok(TileStream::empty()));

		let items: Vec<(TileCoord, Result<Blob>)> = flat.to_vec().await;
		assert_eq!(items.len(), 0);
	}

	#[tokio::test]
	async fn test_flat_map_parallel_try_multiple_inputs() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 0, 0), Blob::from("b"))]);

		let flat = stream.flat_map_parallel_try(|coord, blob| {
			// Each input produces 2 outputs
			Ok(TileStream::from_vec(vec![
				(coord, Blob::from(format!("{}-1", blob.as_str()))),
				(coord, Blob::from(format!("{}-2", blob.as_str()))),
			]))
		});

		let items: Vec<(TileCoord, Blob)> = flat.unwrap_results().to_vec().await;
		assert_eq!(items.len(), 4); // 2 inputs × 2 outputs each
	}
}
