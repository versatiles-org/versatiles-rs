//! Inspect methods for TileStream.
//!
//! This module provides `inspect` methods to observe tiles without modifying them:
//!
//! | Method | Callback | Execution |
//! |--------|----------|-----------|
//! | `inspect` | `FnMut(TileCoord, &T)` | sync, sequential |
//! | `inspect_parallel` | `Fn(TileCoord, &T)` | sync, parallel |
//! | `inspect_async` | `FnMut(TileCoord, &T) -> impl Future<Output = ()>` | async, sequential |
//! | `inspect_parallel_async` | `FnMut(TileCoord, &T) -> impl Future<Output = ()>` | async, parallel |
//!
//! Unlike other transformation methods, inspect methods receive `&T` (by reference)
//! since they only observe the data without consuming or modifying it.

use super::{Arc, ConcurrencyLimits, Future, StreamExt, TileCoord, TileStream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Sync Sequential
	// -------------------------------------------------------------------------

	/// Observes each tile passing through the stream without modifying it.
	///
	/// This method is useful for side effects like progress tracking, logging, or metrics collection.
	/// The callback receives the coordinate and a reference to the tile data.
	/// The stream passes through unchanged.
	///
	/// # Use Cases
	///
	/// - **Progress tracking**: Increment a counter to show processing progress
	/// - **Logging**: Log tile coordinates as they pass through
	/// - **Debugging**: Inspect tile data without affecting the pipeline
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, &T)` for observation.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::{TileCoord, TileStream, Blob};
	/// use std::sync::atomic::{AtomicUsize, Ordering};
	/// use std::sync::Arc;
	///
	/// # async fn example() {
	/// let counter = Arc::new(AtomicUsize::new(0));
	/// let counter_clone = Arc::clone(&counter);
	///
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("a")),
	///     (TileCoord::new(1, 0, 0).unwrap(), Blob::from("b")),
	///     (TileCoord::new(2, 0, 0).unwrap(), Blob::from("c")),
	/// ]);
	///
	/// // Track progress as items are processed
	/// let result = stream
	///     .inspect(move |_coord, _blob| {
	///         counter_clone.fetch_add(1, Ordering::Relaxed);
	///     })
	///     .to_vec()
	///     .await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 3);
	/// assert_eq!(result.len(), 3);
	/// # }
	/// ```
	pub fn inspect<F>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord, &T) + Send + 'a,
	{
		TileStream {
			inner: self
				.inner
				.map(move |(coord, item)| {
					callback(coord, &item);
					(coord, item)
				})
				.boxed(),
		}
	}

	// -------------------------------------------------------------------------
	// Sync Parallel
	// -------------------------------------------------------------------------

	/// Observes each tile in parallel without modifying it.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. Order of observation
	/// is not guaranteed, but the stream output order matches the input.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, &T)` for observation.
	///   Must be `Send + Sync + 'static` for parallel execution.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::{TileCoord, TileStream, Blob};
	/// use std::sync::atomic::{AtomicUsize, Ordering};
	/// use std::sync::Arc;
	///
	/// # async fn example() {
	/// let counter = Arc::new(AtomicUsize::new(0));
	/// let counter_clone = Arc::clone(&counter);
	///
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("a")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("b")),
	/// ]);
	///
	/// let result = stream
	///     .inspect_parallel(move |_coord, _blob| {
	///         counter_clone.fetch_add(1, Ordering::Relaxed);
	///     })
	///     .to_vec()
	///     .await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 2);
	/// # }
	/// ```
	pub fn inspect_parallel<F>(self, callback: F) -> Self
	where
		F: Fn(TileCoord, &T) + Send + Sync + 'static,
		T: Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || {
					cb(coord, &item);
					(coord, item)
				})
			})
			.buffer_unordered(limits.cpu_bound)
			.map(|result| match result {
				Ok((coord, item)) => (coord, item),
				Err(e) => panic!("Spawned task panicked: {e}"),
			});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Async Sequential
	// -------------------------------------------------------------------------

	/// Observes each tile using an asynchronous callback, processing sequentially.
	///
	/// Each tile is observed one at a time, awaiting the callback before processing
	/// the next tile.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, &T)` for observation.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::{TileCoord, TileStream, Blob};
	/// use std::sync::atomic::{AtomicUsize, Ordering};
	/// use std::sync::Arc;
	///
	/// # async fn example() {
	/// let counter = Arc::new(AtomicUsize::new(0));
	/// let counter_clone = Arc::clone(&counter);
	///
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("a")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("b")),
	/// ]);
	///
	/// let result = stream
	///     .inspect_async(move |_coord, _blob| {
	///         let c = Arc::clone(&counter_clone);
	///         async move {
	///             c.fetch_add(1, Ordering::Relaxed);
	///         }
	///     })
	///     .to_vec()
	///     .await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 2);
	/// # }
	/// ```
	pub fn inspect_async<F, Fut>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord, &T) -> Fut + Send + 'a,
		Fut: Future<Output = ()> + Send + 'a,
		T: Sync + 'a,
	{
		let s = self.inner.then(move |(coord, item)| {
			let fut = callback(coord, &item);
			async move {
				fut.await;
				(coord, item)
			}
		});
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Async Parallel
	// -------------------------------------------------------------------------

	/// Observes each tile in parallel using an asynchronous callback.
	///
	/// Processes multiple tiles concurrently up to the CPU-bound concurrency limit.
	/// Order of observation is not guaranteed, but the stream continues processing.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, &T)` for observation.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::{TileCoord, TileStream, Blob};
	/// use std::sync::atomic::{AtomicUsize, Ordering};
	/// use std::sync::Arc;
	///
	/// # async fn example() {
	/// let counter = Arc::new(AtomicUsize::new(0));
	/// let counter_clone = Arc::clone(&counter);
	///
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("a")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("b")),
	/// ]);
	///
	/// let result = stream
	///     .inspect_parallel_async(move |_coord, _blob| {
	///         let c = Arc::clone(&counter_clone);
	///         async move {
	///             tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
	///             c.fetch_add(1, Ordering::Relaxed);
	///         }
	///     })
	///     .to_vec()
	///     .await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 2);
	/// # }
	/// ```
	pub fn inspect_parallel_async<F, Fut>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord, &T) -> Fut + Send + 'a,
		Fut: Future<Output = ()> + Send + 'a,
		T: Sync + 'a,
	{
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let fut = callback(coord, &item);
				async move {
					fut.await;
					(coord, item)
				}
			})
			.buffer_unordered(limits.cpu_bound);
		TileStream { inner: s.boxed() }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Blob;
	use std::sync::Mutex;
	use std::sync::atomic::{AtomicUsize, Ordering};

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	// -------------------------------------------------------------------------
	// inspect (sync, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_inspect() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = Arc::clone(&counter);

		let items = stream
			.inspect(move |_coord, _blob| {
				counter_clone.fetch_add(1, Ordering::Relaxed);
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 2);
		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test]
	async fn test_inspect_receives_data() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("data0")),
			(tc(1, 1, 1), Blob::from("data1")),
		]);

		let collected = Arc::new(Mutex::new(Vec::new()));
		let collected_clone = Arc::clone(&collected);

		let items = stream
			.inspect(move |coord, blob| {
				collected_clone
					.lock()
					.unwrap()
					.push((coord.level, blob.as_str().to_string()));
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 2);
		let observed = collected.lock().unwrap();
		assert_eq!(observed.len(), 2);
		assert_eq!(observed[0], (0, "data0".to_string()));
		assert_eq!(observed[1], (1, "data1".to_string()));
	}

	#[tokio::test]
	async fn test_inspect_preserves_order() {
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let mut last_x = None;
		let stream = stream.inspect(move |coord, _val| {
			if let Some(prev) = last_x {
				assert!(coord.x > prev, "Items should be in order");
			}
			last_x = Some(coord.x);
		});

		let items = stream.to_vec().await;
		assert_eq!(items.len(), 10);
	}

	// -------------------------------------------------------------------------
	// inspect_parallel (sync, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_inspect_parallel() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = Arc::clone(&counter);

		let items = stream
			.inspect_parallel(move |_coord, _blob| {
				counter_clone.fetch_add(1, Ordering::Relaxed);
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 2);
		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_inspect_parallel_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());

		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		let items = stream
			.inspect_parallel(move |_coord, _item| {
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
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 6);
		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
	}

	// -------------------------------------------------------------------------
	// inspect_async (async, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_inspect_async() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = Arc::clone(&counter);

		let items = stream
			.inspect_async(move |_coord, _blob| {
				let c = Arc::clone(&counter_clone);
				async move {
					c.fetch_add(1, Ordering::Relaxed);
				}
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 2);
		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test]
	async fn test_inspect_async_preserves_order() {
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let collected = Arc::new(Mutex::new(Vec::new()));
		let collected_clone = Arc::clone(&collected);

		let items = stream
			.inspect_async(move |coord, _val| {
				let c = Arc::clone(&collected_clone);
				async move {
					c.lock().unwrap().push(coord.x);
				}
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 10);
		let observed = collected.lock().unwrap();
		for (i, &x) in observed.iter().enumerate() {
			assert_eq!(x, u32::try_from(i).unwrap(), "Items should be observed in order");
		}
	}

	// -------------------------------------------------------------------------
	// inspect_parallel_async (async, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_inspect_parallel_async() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = Arc::clone(&counter);

		let items = stream
			.inspect_parallel_async(move |_coord, _blob| {
				let c = Arc::clone(&counter_clone);
				async move {
					tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
					c.fetch_add(1, Ordering::Relaxed);
				}
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 2);
		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_inspect_parallel_async_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());

		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		let items = stream
			.inspect_parallel_async(move |_coord, _item| {
				let counter = counter_clone.clone();
				let max_parallel = max_parallel_clone.clone();
				let current_parallel = current_parallel_clone.clone();

				async move {
					let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
					loop {
						let max = max_parallel.load(Ordering::SeqCst);
						if prev + 1 > max {
							max_parallel.store(prev + 1, Ordering::SeqCst);
						} else {
							break;
						}
					}
					tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
					current_parallel.fetch_sub(1, Ordering::SeqCst);
					counter.fetch_add(1, Ordering::SeqCst);
				}
			})
			.to_vec()
			.await;

		assert_eq!(items.len(), 6);
		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
	}
}
