//! Map transformation methods for TileStream.
//!
//! This module provides various `map` methods to transform tile data:
//!
//! | Method | Callback | Execution |
//! |--------|----------|-----------|
//! | `map` | `Fn(TileCoord, T) -> O` | sync, sequential |
//! | `map_try` | `Fn(TileCoord, T) -> Result<O>` | sync, sequential |
//! | `map_parallel` | `Fn(TileCoord, T) -> O` | sync, parallel |
//! | `map_parallel_try` | `Fn(TileCoord, T) -> Result<O>` | sync, parallel |
//! | `map_async` | `FnMut(TileCoord, T) -> impl Future<Output = O>` | async, sequential |
//! | `map_async_try` | `FnMut(TileCoord, T) -> impl Future<Output = Result<O>>` | async, sequential |
//! | `map_parallel_async` | `FnMut(TileCoord, T) -> impl Future<Output = O>` | async, parallel |
//! | `map_parallel_async_try` | `FnMut(TileCoord, T) -> impl Future<Output = Result<O>>` | async, parallel |

use super::{Arc, ConcurrencyLimits, Future, Result, StreamExt, TileCoord, TileStream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Sync Sequential
	// -------------------------------------------------------------------------

	/// Transforms each tile using a synchronous callback.
	///
	/// Processes tiles sequentially in the order they arrive.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns the transformed value.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// let mapped = stream.map(|coord, blob| {
	///     Blob::from(format!("level-{}: {}", coord.level, blob.as_str()))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert_eq!(items[0].1.as_str(), "level-0: data");
	/// # }
	/// ```
	pub fn map<F, O>(self, mut callback: F) -> TileStream<'a, O>
	where
		F: FnMut(TileCoord, T) -> O + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.map(move |(coord, item)| (coord, callback(coord, item)));
		TileStream { inner: s.boxed() }
	}

	/// Transforms each tile using a fallible synchronous callback.
	///
	/// Processes tiles sequentially. If the callback returns an error, it is propagated
	/// as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<O>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// let mapped = stream.map_try(|coord, blob| -> Result<Blob> {
	///     Ok(Blob::from(format!("level-{}: {}", coord.level, blob.as_str())))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert!(items[0].1.is_ok());
	/// # }
	/// ```
	pub fn map_try<F, O>(self, mut callback: F) -> TileStream<'a, Result<O>>
	where
		F: FnMut(TileCoord, T) -> Result<O> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.map(move |(coord, item)| {
			let result = callback(coord, item);
			(
				coord,
				result.map_err(|e| e.context(format!("Failed to process tile at {coord:?}"))),
			)
		});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Sync Parallel
	// -------------------------------------------------------------------------

	/// Transforms each tile in parallel using a synchronous callback.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. Order of results
	/// is not guaranteed.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns the transformed value.
	///   Must be `Send + Sync + 'static` for parallel execution.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped = stream.map_parallel(|coord, blob| {
	///     // CPU-intensive transformation
	///     Blob::from(format!("processed-{}", blob.as_str()))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert_eq!(items.len(), 2);
	/// # }
	/// ```
	pub fn map_parallel<F, O>(self, callback: F) -> TileStream<'a, O>
	where
		F: Fn(TileCoord, T) -> O + Send + Sync + 'static,
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
			.map(|result| match result {
				Ok((coord, item)) => (coord, item),
				Err(e) => panic!("Spawned task panicked: {e}"),
			});
		TileStream { inner: s.boxed() }
	}

	/// Transforms each tile in parallel using a fallible synchronous callback.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. If the callback returns
	/// an error, it is propagated as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<O>`.
	///   Must be `Send + Sync + 'static` for parallel execution.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped = stream.map_parallel_try(|coord, blob| -> Result<Blob> {
	///     Ok(Blob::from(format!("processed-{}", blob.as_str())))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert_eq!(items.len(), 2);
	/// # }
	/// ```
	pub fn map_parallel_try<F, O>(self, callback: F) -> TileStream<'a, Result<O>>
	where
		F: Fn(TileCoord, T) -> Result<O> + Send + Sync + 'static,
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
			.map(|result| match result {
				Ok((coord, Ok(item))) => (coord, Ok(item)),
				Ok((coord, Err(e))) => (coord, Err(e.context(format!("Failed to process tile at {coord:?}")))),
				Err(e) => panic!("Spawned task panicked: {e}"),
			});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Async Sequential
	// -------------------------------------------------------------------------

	/// Transforms each tile using an asynchronous callback, processing sequentially.
	///
	/// Each tile is processed one at a time, awaiting the callback before processing
	/// the next tile.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns the transformed value.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// let mapped = stream.map_async(|coord, blob| async move {
	///     // Async I/O operation
	///     Blob::from(format!("fetched-{}", blob.as_str()))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert_eq!(items[0].1.as_str(), "fetched-data");
	/// # }
	/// ```
	pub fn map_async<F, Fut, O>(self, mut callback: F) -> TileStream<'a, O>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = O> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.then(move |(coord, item)| {
			let fut = callback(coord, item);
			async move { (coord, fut.await) }
		});
		TileStream { inner: s.boxed() }
	}

	/// Transforms each tile using a fallible asynchronous callback, processing sequentially.
	///
	/// Each tile is processed one at a time. If the callback returns an error, it is
	/// propagated as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Result<O>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// let mapped = stream.map_async_try(|coord, blob| async move {
	///     Ok(Blob::from(format!("fetched-{}", blob.as_str())))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert!(items[0].1.is_ok());
	/// # }
	/// ```
	pub fn map_async_try<F, Fut, O>(self, mut callback: F) -> TileStream<'a, Result<O>>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = Result<O>> + Send + 'a,
		O: Send + 'a,
	{
		let s = self.inner.then(move |(coord, item)| {
			let fut = callback(coord, item);
			async move {
				let result = fut.await;
				(
					coord,
					result.map_err(|e| e.context(format!("Failed to process tile at {coord:?}"))),
				)
			}
		});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Async Parallel
	// -------------------------------------------------------------------------

	/// Transforms each tile in parallel using an asynchronous callback.
	///
	/// Processes multiple tiles concurrently up to the CPU-bound concurrency limit.
	/// Order of results is not guaranteed.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns the transformed value.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # use futures::StreamExt;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped = stream.map_parallel_async(|coord, blob| async move {
	///     tokio::time::sleep(std::time::Duration::from_millis(1)).await;
	///     Blob::from(format!("processed-{}-{}", coord.level, blob.as_str()))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert_eq!(items.len(), 2);
	/// # }
	/// ```
	pub fn map_parallel_async<F, Fut, O>(self, mut callback: F) -> TileStream<'a, O>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = O> + Send + 'a,
		O: Send + 'a,
	{
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let fut = callback(coord, item);
				async move { (coord, fut.await) }
			})
			.buffer_unordered(limits.cpu_bound);
		TileStream { inner: s.boxed() }
	}

	/// Transforms each tile in parallel using a fallible asynchronous callback.
	///
	/// Processes multiple tiles concurrently up to the CPU-bound concurrency limit.
	/// If the callback returns an error, it is propagated as an item in the stream.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Result<O>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped = stream.map_parallel_async_try(|coord, blob| async move {
	///     tokio::time::sleep(std::time::Duration::from_millis(1)).await;
	///     Ok(Blob::from(format!("processed-{}-{}", coord.level, blob.as_str())))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// assert_eq!(items.len(), 2);
	/// # }
	/// ```
	pub fn map_parallel_async_try<F, Fut, O>(self, mut callback: F) -> TileStream<'a, Result<O>>
	where
		F: FnMut(TileCoord, T) -> Fut + Send + 'a,
		Fut: Future<Output = Result<O>> + Send + 'a,
		O: Send + 'a,
	{
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let fut = callback(coord, item);
				async move {
					let result = fut.await;
					(
						coord,
						result.map_err(|e| e.context(format!("Failed to process tile at {coord:?}"))),
					)
				}
			})
			.buffer_unordered(limits.cpu_bound);
		TileStream { inner: s.boxed() }
	}
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::Blob;
	use anyhow::bail;
	use std::sync::atomic::{AtomicUsize, Ordering};

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	// -------------------------------------------------------------------------
	// map (sync, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let mapped = stream.map(|coord, blob| Blob::from(format!("{}:{}", coord.level, blob.as_str())));

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:a");
		assert_eq!(items[1].1.as_str(), "1:b");
	}

	#[tokio::test]
	async fn test_map_preserves_order() {
		// Use level 10 to allow x values 0..10 (level n has 2^n tiles per dimension)
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let mapped = stream.map(|_coord, val| val * 2);

		let items = mapped.to_vec().await;
		for (i, (coord, val)) in items.iter().enumerate() {
			assert_eq!(coord.x, i as u32);
			assert_eq!(*val, (i as u32) * 2);
		}
	}

	// -------------------------------------------------------------------------
	// map_try (sync, sequential, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let mapped = stream.map_try(|coord, blob| Ok(Blob::from(format!("{}:{}", coord.level, blob.as_str()))));

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_ok());
		assert_eq!(items[0].1.as_ref().unwrap().as_str(), "0:data");
	}

	#[tokio::test]
	async fn test_map_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let mapped = stream.map_try(|_coord, _blob| -> Result<Blob> { bail!("test error") });

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
		// The error has context added, so check the full error chain
		let err = items[0].1.as_ref().unwrap_err();
		let err_string = format!("{err:?}");
		assert!(
			err_string.contains("test error"),
			"Expected 'test error' in: {err_string}"
		);
	}

	// -------------------------------------------------------------------------
	// map_parallel (sync, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_parallel() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let mapped = stream.map_parallel(|coord, blob| Blob::from(format!("{}:{}", coord.level, blob.as_str())));

		let mut items = mapped.to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:a");
		assert_eq!(items[1].1.as_str(), "1:b");
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_map_parallel_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());
		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		let stream = stream.map_parallel(move |_coord, item| {
			let counter = counter_clone.clone();
			let max_parallel = max_parallel_clone.clone();
			let current_parallel = current_parallel_clone.clone();

			let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
			loop {
				let max = max_parallel.load(Ordering::SeqCst);
				if prev + 1 > max {
					max_parallel.store(prev + 1, Ordering::SeqCst);
				} else {
					break;
				}
			}
			std::thread::sleep(std::time::Duration::from_millis(10));
			current_parallel.fetch_sub(1, Ordering::SeqCst);
			counter.fetch_add(1, Ordering::SeqCst);
			item
		});

		let results = stream.to_vec().await;
		assert_eq!(results.len(), 6);
		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
	}

	// -------------------------------------------------------------------------
	// map_parallel_try (sync, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_parallel_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let mapped = stream.map_parallel_try(|coord, blob| Ok(Blob::from(format!("{}:{}", coord.level, blob.as_str()))));

		let mut items = mapped.to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert!(items[0].1.is_ok());
		assert_eq!(items[0].1.as_ref().unwrap().as_str(), "0:a");
	}

	#[tokio::test]
	async fn test_map_parallel_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let mapped = stream.map_parallel_try(|_coord, _blob| -> Result<Blob> { bail!("test error") });

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}

	// -------------------------------------------------------------------------
	// map_async (async, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_async() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let mapped =
			stream.map_async(|coord, blob| async move { Blob::from(format!("{}:{}", coord.level, blob.as_str())) });

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:a");
		assert_eq!(items[1].1.as_str(), "1:b");
	}

	#[tokio::test]
	async fn test_map_async_preserves_order() {
		// Use level 10 to allow x values 0..10 (level n has 2^n tiles per dimension)
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let mapped = stream.map_async(|_coord, val| async move { val * 2 });

		let items = mapped.to_vec().await;
		for (i, (coord, val)) in items.iter().enumerate() {
			assert_eq!(coord.x, i as u32);
			assert_eq!(*val, (i as u32) * 2);
		}
	}

	// -------------------------------------------------------------------------
	// map_async_try (async, sequential, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_async_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let mapped = stream
			.map_async_try(|coord, blob| async move { Ok(Blob::from(format!("{}:{}", coord.level, blob.as_str()))) });

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_ok());
	}

	#[tokio::test]
	async fn test_map_async_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let mapped = stream.map_async_try(|_coord, _blob| async move { bail!("test error") });

		let items: Vec<(TileCoord, Result<Blob>)> = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}

	// -------------------------------------------------------------------------
	// map_parallel_async (async, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_parallel_async_basic() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let mapped = stream
			.map_parallel_async(|coord, blob| async move { Blob::from(format!("{}:{}", coord.level, blob.as_str())) });

		let mut items = mapped.to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "0:a");
		assert_eq!(items[1].1.as_str(), "1:b");
	}

	#[tokio::test]
	async fn test_map_parallel_async_with_delay() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("data0")),
			(tc(1, 1, 1), Blob::from("data1")),
		]);

		let mapped = stream.map_parallel_async(|coord, blob| async move {
			tokio::time::sleep(std::time::Duration::from_millis(1)).await;
			Blob::from(format!("processed-{}-{}", coord.level, blob.as_str()))
		});

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 2);
	}

	// -------------------------------------------------------------------------
	// map_parallel_async_try (async, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_parallel_async_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let mapped = stream.map_parallel_async_try(|coord, blob| async move {
			Ok(Blob::from(format!("{}:{}", coord.level, blob.as_str())))
		});

		let mut items = mapped.to_vec().await;
		items.sort_by_key(|(c, _)| c.level);
		assert_eq!(items.len(), 2);
		assert!(items[0].1.is_ok());
	}

	#[tokio::test]
	async fn test_map_parallel_async_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let mapped = stream.map_parallel_async_try(|_coord, _blob| async move { bail!("test error") });

		let items: Vec<(TileCoord, Result<Blob>)> = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		assert!(items[0].1.is_err());
	}
}
