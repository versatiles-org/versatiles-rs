//! Filter-map transformation methods for TileStream.
//!
//! This module provides `filter_map` methods that combine filtering and transformation:
//!
//! | Method | Callback | Execution |
//! |--------|----------|-----------|
//! | `filter_map_parallel_try` | `Fn(TileCoord, T) -> Result<Option<O>>` | sync, parallel |

use super::{Arc, ConcurrencyLimits, Result, StreamExt, TileCoord, TileStream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Sync Parallel
	// -------------------------------------------------------------------------

	/// Filters and transforms each tile in parallel, discarding items where `callback` returns `None`.
	///
	/// Spawns tokio tasks with CPU-bound concurrency limit. Each item `(coord, value)` is mapped
	/// to `(coord, Result<callback(coord, value)>)`. If `callback` returns `Ok(None)`, the item is dropped.
	/// If it returns `Ok(Some(value))`, the item is kept. If it returns `Err`, the error is propagated.
	///
	/// Returns a stream of `Result<O>` values. If the callback returns an error for any tile,
	/// that error is propagated as an item in the stream. Items where the callback returns `Ok(None)`
	/// are filtered out.
	///
	/// Uses CPU-bound concurrency limit since the callback runs in `spawn_blocking`.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<Option<O>>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # use futures::{StreamExt, TryStreamExt};
	/// # async fn example() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_parallel_try(|coord, value| {
	///     Ok(if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     })
	/// });
	///
	/// // Collect results, failing fast on first error
	/// let items: Vec<(TileCoord, Blob)> = filtered
	///     .inner
	///     .filter_map(|(coord, result)| async move {
	///         match result {
	///             Ok(item) => Some(Ok((coord, item))),
	///             Err(e) => Some(Err(e)),
	///         }
	///     })
	///     .try_collect()
	///     .await?;
	/// assert_eq!(items.len(), 1);
	/// # Ok(())
	/// # }
	/// ```
	pub fn filter_map_parallel_try<F, O>(self, callback: F) -> TileStream<'a, Result<O>>
	where
		F: Fn(TileCoord, T) -> Result<Option<O>> + Send + Sync + 'static,
		T: 'static,
		O: Send + Sync + 'static,
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
			.filter_map(|result| async move {
				match result {
					Ok((coord, Ok(Some(item)))) => Some((coord, Ok(item))),
					Ok((_coord, Ok(None))) => None, // Filter out None results
					Ok((coord, Err(e))) => Some((coord, Err(e.context(format!("Failed to process tile at {coord:?}"))))),
					Err(e) => panic!("Spawned task panicked: {e}"), // Task panic is still a panic (unexpected)
				}
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
	// filter_map_parallel_try (sync, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_parallel_try_keeps_some() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
			(tc(2, 2, 2), Blob::from("keep")),
		]);

		let filtered = stream.filter_map_parallel_try(|coord, blob| {
			Ok(if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			})
		});

		let mut items: Vec<(TileCoord, Blob)> = filtered.unwrap_results().to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:keep");
		assert_eq!(items[1].1.as_str(), "2:keep");
	}

	#[tokio::test]
	async fn test_filter_map_parallel_try_discards_all() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let filtered = stream.filter_map_parallel_try(|_coord, _blob| Ok::<Option<Blob>, anyhow::Error>(None));

		let items: Vec<(TileCoord, Result<Blob>)> = filtered.to_vec().await;
		assert_eq!(items.len(), 0);
	}

	#[tokio::test]
	async fn test_filter_map_parallel_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let filtered = stream.filter_map_parallel_try(|_coord, _blob| bail!("test error"));

		let items: Vec<(TileCoord, Result<Blob>)> = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}

	#[tokio::test]
	async fn test_filter_map_parallel_try_receives_coord() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(5, 10, 20), Blob::from("b"))]);

		let filtered = stream.filter_map_parallel_try(|coord, _blob| {
			// Only keep tiles at level 5
			Ok(if coord.level == 5 {
				Some(Blob::from(format!("kept-{}-{}", coord.x, coord.y)))
			} else {
				None
			})
		});

		let items: Vec<(TileCoord, Blob)> = filtered.unwrap_results().to_vec().await;
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].0, tc(5, 10, 20));
		assert_eq!(items[0].1.as_str(), "kept-10-20");
	}
}
