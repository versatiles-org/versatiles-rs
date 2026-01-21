//! Filter-map transformation methods for TileStream.
//!
//! This module provides `filter_map` methods that combine filtering and transformation:
//!
//! | Method | Callback | Execution |
//! |--------|----------|-----------|
//! | `filter_map` | `Fn(TileCoord, T) -> Option<O>` | sync, sequential |
//! | `filter_map_try` | `Fn(TileCoord, T) -> Result<Option<O>>` | sync, sequential |
//! | `filter_map_parallel` | `Fn(TileCoord, T) -> Option<O>` | sync, parallel |
//! | `filter_map_parallel_try` | `Fn(TileCoord, T) -> Result<Option<O>>` | sync, parallel |
//! | `filter_map_async` | `FnMut(TileCoord, T) -> impl Future<Output = Option<O>>` | async, sequential |
//! | `filter_map_async_try` | `FnMut(TileCoord, T) -> impl Future<Output = Result<Option<O>>>` | async, sequential |
//! | `filter_map_parallel_async` | `FnMut(TileCoord, T) -> impl Future<Output = Option<O>>` | async, parallel |
//! | `filter_map_parallel_async_try` | `FnMut(TileCoord, T) -> impl Future<Output = Result<Option<O>>>` | async, parallel |

use super::{Arc, ConcurrencyLimits, Future, Result, StreamExt, TileCoord, TileStream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Sync Sequential
	// -------------------------------------------------------------------------

	/// Filters and transforms each tile sequentially, discarding items where `callback` returns `None`.
	///
	/// Processes tiles in order. Items where the callback returns `None` are filtered out.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Option<O>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map(|coord, value| {
	///     if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     }
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map<F, O>(self, mut callback: F) -> TileStream<'a, O>
	where
		F: FnMut(TileCoord, T) -> Option<O> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.filter_map(move |(coord, item)| {
			let result = callback(coord, item);
			async move { result.map(|o| (coord, o)) }
		});
		TileStream { inner: s.boxed() }
	}

	/// Filters and transforms each tile sequentially with error handling.
	///
	/// Processes tiles in order. If the callback returns `Ok(None)`, the item is filtered out.
	/// If it returns `Ok(Some(value))`, the item is kept. If it returns `Err`, the error is
	/// propagated as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<Option<O>>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_try(|coord, value| -> Result<Option<Blob>> {
	///     Ok(if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     })
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_try<F, O>(self, mut callback: F) -> TileStream<'a, Result<O>>
	where
		F: FnMut(TileCoord, T) -> Result<Option<O>> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.filter_map(move |(coord, item)| {
			let result = callback(coord, item);
			async move {
				match result {
					Ok(Some(o)) => Some((coord, Ok(o))),
					Ok(None) => None,
					Err(e) => Some((coord, Err(e.context(format!("Failed to process tile at {coord:?}"))))),
				}
			}
		});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Sync Parallel
	// -------------------------------------------------------------------------

	/// Filters and transforms each tile in parallel, discarding items where `callback` returns `None`.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. Order of results is not guaranteed.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Option<O>`.
	///   Must be `Send + Sync + 'static` for parallel execution.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_parallel(|coord, value| {
	///     if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     }
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_parallel<F, O>(self, callback: F) -> TileStream<'a, O>
	where
		F: Fn(TileCoord, T) -> Option<O> + Send + Sync + 'static,
		T: 'static,
		O: Send + 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(coord, item)))
			})
			.buffer_unordered(limits.cpu_bound)
			.filter_map(|result| async move {
				match result {
					Ok((coord, Some(item))) => Some((coord, item)),
					Ok((_coord, None)) => None,
					Err(e) => panic!("Spawned task panicked: {e}"),
				}
			});
		TileStream { inner: s.boxed() }
	}

	/// Filters and transforms each tile in parallel with error handling.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. If the callback returns
	/// `Ok(None)`, the item is filtered out. If it returns `Err`, the error is propagated.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<Option<O>>`.
	///   Must be `Send + Sync + 'static` for parallel execution.
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
			.buffer_unordered(limits.cpu_bound)
			.filter_map(|result| async move {
				match result {
					Ok((coord, Ok(Some(item)))) => Some((coord, Ok(item))),
					Ok((_coord, Ok(None))) => None,
					Ok((coord, Err(e))) => Some((coord, Err(e.context(format!("Failed to process tile at {coord:?}"))))),
					Err(e) => panic!("Spawned task panicked: {e}"),
				}
			});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Async Sequential
	// -------------------------------------------------------------------------

	/// Filters and transforms each tile using an async callback, processing sequentially.
	///
	/// Each tile is processed one at a time. Items where the callback returns `None` are filtered out.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Option<O>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_async(|coord, value| async move {
	///     if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     }
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_async<F, Fut, O>(self, mut callback: F) -> TileStream<'a, O>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = Option<O>> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.filter_map(move |(coord, item)| {
			let fut = callback(coord, item);
			async move { fut.await.map(|o| (coord, o)) }
		});
		TileStream { inner: s.boxed() }
	}

	/// Filters and transforms each tile using an async callback with error handling.
	///
	/// Each tile is processed one at a time. If the callback returns `Ok(None)`, the item is
	/// filtered out. If it returns `Err`, the error is propagated as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Result<Option<O>>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_async_try(|coord, value| async move {
	///     Ok(if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     })
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_async_try<F, Fut, O>(self, mut callback: F) -> TileStream<'a, Result<O>>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = Result<Option<O>>> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.filter_map(move |(coord, item)| {
			let fut = callback(coord, item);
			async move {
				match fut.await {
					Ok(Some(o)) => Some((coord, Ok(o))),
					Ok(None) => None,
					Err(e) => Some((coord, Err(e.context(format!("Failed to process tile at {coord:?}"))))),
				}
			}
		});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Async Parallel
	// -------------------------------------------------------------------------

	/// Filters and transforms each tile in parallel using an async callback.
	///
	/// Processes multiple tiles concurrently up to the CPU-bound concurrency limit.
	/// Order of results is not guaranteed. Items where the callback returns `None` are filtered out.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Option<O>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_parallel_async(|coord, value| async move {
	///     if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     }
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_parallel_async<F, Fut, O>(self, mut callback: F) -> TileStream<'a, O>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = Option<O>> + Send + 'a,
		O: Send + 'a,
	{
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let fut = callback(coord, item);
				async move { (coord, fut.await) }
			})
			.buffer_unordered(limits.cpu_bound)
			.filter_map(|(coord, result)| async move { result.map(|o| (coord, o)) });
		TileStream { inner: s.boxed() }
	}

	/// Filters and transforms each tile in parallel using an async callback with error handling.
	///
	/// Processes multiple tiles concurrently up to the CPU-bound concurrency limit.
	/// If the callback returns `Ok(None)`, the item is filtered out. If it returns `Err`,
	/// the error is propagated as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Result<Option<O>>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_parallel_async_try(|coord, value| async move {
	///     Ok(if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("level-{}:{}", coord.level, value.as_str())))
	///     })
	/// });
	///
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_parallel_async_try<F, Fut, O>(self, mut callback: F) -> TileStream<'a, Result<O>>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = Result<Option<O>>> + Send + 'a,
		O: Send + 'a,
	{
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let fut = callback(coord, item);
				async move { (coord, fut.await) }
			})
			.buffer_unordered(limits.cpu_bound)
			.filter_map(|(coord, result)| async move {
				match result {
					Ok(Some(o)) => Some((coord, Ok(o))),
					Ok(None) => None,
					Err(e) => Some((coord, Err(e.context(format!("Failed to process tile at {coord:?}"))))),
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
	// filter_map (sync, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
			(tc(2, 2, 2), Blob::from("keep")),
		]);

		let filtered = stream.filter_map(|coord, blob| {
			if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			}
		});

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:keep");
		assert_eq!(items[1].1.as_str(), "2:keep");
	}

	#[tokio::test]
	async fn test_filter_map_preserves_order() {
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let filtered = stream.filter_map(|_coord, val| if val % 2 == 0 { Some(val) } else { None });

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 5);
		for (i, (coord, val)) in items.iter().enumerate() {
			let expected = u32::try_from(i * 2).unwrap();
			assert_eq!(coord.x, expected);
			assert_eq!(*val, expected);
		}
	}

	// -------------------------------------------------------------------------
	// filter_map_try (sync, sequential, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_try_success() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
		]);

		let filtered = stream.filter_map_try(|coord, blob| {
			Ok(if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			})
		});

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_ok());
	}

	#[tokio::test]
	async fn test_filter_map_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let filtered = stream.filter_map_try(|_coord, _blob| -> Result<Option<Blob>> { bail!("test error") });

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}

	// -------------------------------------------------------------------------
	// filter_map_parallel (sync, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_parallel() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
			(tc(2, 2, 2), Blob::from("keep")),
		]);

		let filtered = stream.filter_map_parallel(|coord, blob| {
			if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			}
		});

		let mut items = filtered.to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:keep");
		assert_eq!(items[1].1.as_str(), "2:keep");
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

	// -------------------------------------------------------------------------
	// filter_map_async (async, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_async() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
		]);

		let filtered = stream.filter_map_async(|coord, blob| async move {
			if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			}
		});

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].1.as_str(), "0:keep");
	}

	// -------------------------------------------------------------------------
	// filter_map_async_try (async, sequential, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_async_try_success() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
		]);

		let filtered = stream.filter_map_async_try(|coord, blob| async move {
			Ok(if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			})
		});

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_ok());
	}

	#[tokio::test]
	async fn test_filter_map_async_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let filtered = stream.filter_map_async_try(|_coord, _blob| async move { bail!("test error") });

		let items: Vec<(TileCoord, Result<Blob>)> = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}

	// -------------------------------------------------------------------------
	// filter_map_parallel_async (async, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_parallel_async() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
			(tc(2, 2, 2), Blob::from("keep")),
		]);

		let filtered = stream.filter_map_parallel_async(|coord, blob| async move {
			if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			}
		});

		let mut items = filtered.to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:keep");
		assert_eq!(items[1].1.as_str(), "2:keep");
	}

	// -------------------------------------------------------------------------
	// filter_map_parallel_async_try (async, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_map_parallel_async_try_success() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("keep")),
			(tc(1, 1, 1), Blob::from("discard")),
		]);

		let filtered = stream.filter_map_parallel_async_try(|coord, blob| async move {
			Ok(if blob.as_str() == "discard" {
				None
			} else {
				Some(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
			})
		});

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_ok());
	}

	#[tokio::test]
	async fn test_filter_map_parallel_async_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let filtered = stream.filter_map_parallel_async_try(|_coord, _blob| async move { bail!("test error") });

		let items: Vec<(TileCoord, Result<Blob>)> = filtered.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}
}
