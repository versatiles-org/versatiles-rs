//! For-each consumption methods for TileStream.
//!
//! This module provides various `for_each` methods to consume and process tiles:
//!
//! | Method | Callback | Execution |
//! |--------|----------|-----------|
//! | `for_each` | `FnMut(TileCoord, T)` | sync, sequential |
//! | `for_each_try` | `FnMut(TileCoord, T) -> Result<()>` | sync, sequential |
//! | `for_each_parallel` | `Fn(TileCoord, T)` | sync, parallel |
//! | `for_each_parallel_try` | `Fn(TileCoord, T) -> Result<()>` | sync, parallel |
//! | `for_each_async` | `FnMut(TileCoord, T) -> impl Future<Output = ()>` | async, sequential |
//! | `for_each_async_try` | `FnMut(TileCoord, T) -> impl Future<Output = Result<()>>` | async, sequential |
//! | `for_each_parallel_async` | `FnMut(TileCoord, T) -> impl Future<Output = ()>` | async, parallel |
//! | `for_each_parallel_async_try` | `FnMut(TileCoord, T) -> impl Future<Output = Result<()>>` | async, parallel |

use super::{Arc, ConcurrencyLimits, Future, Result, StreamExt, TileCoord, TileStream, ready};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Sync Sequential
	// -------------------------------------------------------------------------

	/// Applies a synchronous callback to each tile, consuming the stream.
	///
	/// Processes tiles sequentially in the order they arrive.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)`.
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
	/// stream.for_each(|coord, blob| {
	///     println!("Processing tile at level {}: {}", coord.level, blob.as_str());
	/// }).await;
	/// # }
	/// ```
	pub async fn for_each<F>(self, mut callback: F)
	where
		F: FnMut(TileCoord, T),
	{
		self
			.inner
			.for_each(|(coord, item)| {
				callback(coord, item);
				ready(())
			})
			.await;
	}

	/// Applies a fallible synchronous callback to each tile, consuming the stream.
	///
	/// Processes tiles sequentially. Returns an error if any callback fails.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<()>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// stream.for_each_try(|coord, blob| {
	///     if blob.is_empty() {
	///         anyhow::bail!("Empty blob at {:?}", coord);
	///     }
	///     println!("Valid tile at {:?}", coord);
	///     Ok(())
	/// }).await?;
	/// # Ok(())
	/// # }
	/// ```
	pub async fn for_each_try<F>(self, mut callback: F) -> Result<()>
	where
		F: FnMut(TileCoord, T) -> Result<()>,
	{
		let mut result = Ok(());
		self
			.inner
			.for_each(|(coord, item)| {
				if result.is_ok() {
					result = callback(coord, item).map_err(|e| e.context(format!("Failed to process tile at {coord:?}")));
				}
				ready(())
			})
			.await;
		result
	}

	// -------------------------------------------------------------------------
	// Sync Parallel
	// -------------------------------------------------------------------------

	/// Applies a synchronous callback to each tile in parallel, consuming the stream.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. Order of execution
	/// is not guaranteed.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)`.
	///   Must be `Send + Sync + 'static` for parallel execution.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use std::sync::atomic::{AtomicUsize, Ordering};
	/// # use std::sync::Arc;
	/// # async fn example() {
	/// let counter = Arc::new(AtomicUsize::new(0));
	/// let counter_clone = counter.clone();
	///
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_parallel(move |_coord, _blob| {
	///     counter_clone.fetch_add(1, Ordering::Relaxed);
	/// }).await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 2);
	/// # }
	/// ```
	pub async fn for_each_parallel<F>(self, callback: F)
	where
		F: Fn(TileCoord, T) + Send + Sync + 'static,
		T: 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || cb(coord, item))
			})
			.buffer_unordered(limits.cpu_bound)
			.for_each(|result| {
				if let Err(e) = result {
					panic!("Spawned task panicked: {e}");
				}
				ready(())
			})
			.await;
	}

	/// Applies a fallible synchronous callback to each tile in parallel, consuming the stream.
	///
	/// Spawns blocking tasks with CPU-bound concurrency limit. Returns an error if any
	/// callback fails.
	///
	/// # Arguments
	/// * `callback` - Function that receives `(TileCoord, T)` and returns `Result<()>`.
	///   Must be `Send + Sync + 'static` for parallel execution.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_parallel_try(|coord, blob| {
	///     if blob.is_empty() {
	///         anyhow::bail!("Empty blob at {:?}", coord);
	///     }
	///     Ok(())
	/// }).await?;
	/// # Ok(())
	/// # }
	/// ```
	pub async fn for_each_parallel_try<F>(self, callback: F) -> Result<()>
	where
		F: Fn(TileCoord, T) -> Result<()> + Send + Sync + 'static,
		T: 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let mut result = Ok(());
		self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(coord, item)))
			})
			.buffer_unordered(limits.cpu_bound)
			.for_each(|task_result| {
				match task_result {
					Ok((coord, Err(e))) if result.is_ok() => {
						result = Err(e.context(format!("Failed to process tile at {coord:?}")));
					}
					Err(e) => panic!("Spawned task panicked: {e}"),
					_ => {}
				}
				ready(())
			})
			.await;
		result
	}

	// -------------------------------------------------------------------------
	// Async Sequential
	// -------------------------------------------------------------------------

	/// Applies an asynchronous callback to each tile, consuming the stream.
	///
	/// Processes tiles sequentially in the order they arrive.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)`.
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
	/// stream.for_each_async(|coord, blob| async move {
	///     println!("Processing tile at level {}: {}", coord.level, blob.as_str());
	/// }).await;
	/// # }
	/// ```
	pub async fn for_each_async<F, Fut>(self, mut callback: F)
	where
		F: FnMut(TileCoord, T) -> Fut,
		Fut: Future<Output = ()>,
	{
		self.inner.for_each(|(coord, item)| callback(coord, item)).await;
	}

	/// Applies a fallible asynchronous callback to each tile, consuming the stream.
	///
	/// Processes tiles sequentially. Returns an error if any callback fails.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Result<()>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// stream.for_each_async_try(|coord, blob| async move {
	///     // Async I/O operation that might fail
	///     if blob.is_empty() {
	///         anyhow::bail!("Empty blob at {:?}", coord);
	///     }
	///     Ok(())
	/// }).await?;
	/// # Ok(())
	/// # }
	/// ```
	pub async fn for_each_async_try<F, Fut>(mut self, mut callback: F) -> Result<()>
	where
		F: FnMut(TileCoord, T) -> Fut,
		Fut: Future<Output = Result<()>>,
	{
		while let Some((coord, item)) = self.inner.next().await {
			callback(coord, item)
				.await
				.map_err(|e| e.context(format!("Failed to process tile at {coord:?}")))?;
		}
		Ok(())
	}

	// -------------------------------------------------------------------------
	// Async Parallel
	// -------------------------------------------------------------------------

	/// Applies an asynchronous callback to each tile in parallel, consuming the stream.
	///
	/// Processes multiple tiles concurrently up to the CPU-bound concurrency limit.
	/// Order of execution is not guaranteed.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use std::sync::atomic::{AtomicUsize, Ordering};
	/// # use std::sync::Arc;
	/// # async fn example() {
	/// let counter = Arc::new(AtomicUsize::new(0));
	/// let counter_clone = counter.clone();
	///
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_parallel_async(move |_coord, _blob| {
	///     let c = counter_clone.clone();
	///     async move {
	///         tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
	///         c.fetch_add(1, Ordering::Relaxed);
	///     }
	/// }).await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 2);
	/// # }
	/// ```
	pub async fn for_each_parallel_async<F, Fut>(self, mut callback: F)
	where
		F: FnMut(TileCoord, T) -> Fut,
		Fut: Future<Output = ()>,
	{
		let limits = ConcurrencyLimits::default();
		self
			.inner
			.for_each_concurrent(limits.cpu_bound, |(coord, item)| callback(coord, item))
			.await;
	}

	/// Applies a fallible asynchronous callback to each tile in parallel, consuming the stream.
	///
	/// Processes multiple tiles concurrently. Returns an error if any callback fails.
	///
	/// # Arguments
	/// * `callback` - Async function that receives `(TileCoord, T)` and returns `Result<()>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # async fn example() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_parallel_async_try(|coord, blob| async move {
	///     tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
	///     if blob.is_empty() {
	///         anyhow::bail!("Empty blob at {:?}", coord);
	///     }
	///     Ok(())
	/// }).await?;
	/// # Ok(())
	/// # }
	/// ```
	pub async fn for_each_parallel_async_try<F, Fut>(self, mut callback: F) -> Result<()>
	where
		F: FnMut(TileCoord, T) -> Fut,
		Fut: Future<Output = Result<()>>,
	{
		let limits = ConcurrencyLimits::default();
		let errors = Arc::new(std::sync::Mutex::new(Vec::new()));
		self
			.inner
			.for_each_concurrent(limits.cpu_bound, |(coord, item)| {
				let fut = callback(coord, item);
				let errors_clone = Arc::clone(&errors);
				async move {
					if let Err(e) = fut.await {
						let mut errs = errors_clone.lock().unwrap();
						errs.push(e.context(format!("Failed to process tile at {coord:?}")));
					}
				}
			})
			.await;
		let errs = Arc::try_unwrap(errors).unwrap().into_inner().unwrap();
		if let Some(e) = errs.into_iter().next() {
			Err(e)
		} else {
			Ok(())
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Blob;
	use anyhow::bail;
	use std::sync::Mutex;
	use std::sync::atomic::{AtomicUsize, Ordering};

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	// -------------------------------------------------------------------------
	// for_each (sync, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let collected = Mutex::new(Vec::new());
		stream
			.for_each(|coord, blob| {
				collected.lock().unwrap().push((coord.level, blob.as_str().to_string()));
			})
			.await;

		let items = collected.into_inner().unwrap();
		assert_eq!(items.len(), 2);
		assert_eq!(items[0], (0, "a".to_string()));
		assert_eq!(items[1], (1, "b".to_string()));
	}

	#[tokio::test]
	async fn test_for_each_preserves_order() {
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let collected = Mutex::new(Vec::new());
		stream
			.for_each(|coord, val| {
				collected.lock().unwrap().push((coord.x, val));
			})
			.await;

		let items = collected.into_inner().unwrap();
		for (i, (x, val)) in items.iter().enumerate() {
			let expected = u32::try_from(i).unwrap();
			assert_eq!(*x, expected);
			assert_eq!(*val, expected);
		}
	}

	// -------------------------------------------------------------------------
	// for_each_try (sync, sequential, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let result = stream.for_each_try(|_coord, _blob| Ok(())).await;

		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_for_each_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let result = stream.for_each_try(|_coord, _blob| bail!("test error")).await;

		assert!(result.is_err());
		let err_string = format!("{:?}", result.unwrap_err());
		assert!(err_string.contains("test error"));
	}

	#[tokio::test]
	async fn test_for_each_try_stops_on_first_error() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("a")),
			(tc(1, 1, 1), Blob::from("b")),
			(tc(2, 2, 2), Blob::from("c")),
		]);

		let counter = AtomicUsize::new(0);
		let result = stream
			.for_each_try(|_coord, _blob| {
				let count = counter.fetch_add(1, Ordering::SeqCst);
				if count >= 1 {
					bail!("error at item {count}");
				}
				Ok(())
			})
			.await;

		assert!(result.is_err());
		// Should have processed at least 2 items (the successful one and the failing one)
		assert!(counter.load(Ordering::SeqCst) >= 2);
	}

	// -------------------------------------------------------------------------
	// for_each_parallel (sync, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_parallel() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = counter.clone();

		stream
			.for_each_parallel(move |_coord, _blob| {
				counter_clone.fetch_add(1, Ordering::Relaxed);
			})
			.await;

		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_for_each_parallel_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());

		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		stream
			.for_each_parallel(move |_coord, _item| {
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
			.await;

		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
	}

	// -------------------------------------------------------------------------
	// for_each_parallel_try (sync, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_parallel_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let result = stream.for_each_parallel_try(|_coord, _blob| Ok(())).await;

		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_for_each_parallel_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let result = stream.for_each_parallel_try(|_coord, _blob| bail!("test error")).await;

		assert!(result.is_err());
	}

	// -------------------------------------------------------------------------
	// for_each_async (async, sequential)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_async() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = counter.clone();

		stream
			.for_each_async(move |_coord, _blob| {
				let c = counter_clone.clone();
				async move {
					c.fetch_add(1, Ordering::Relaxed);
				}
			})
			.await;

		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test]
	async fn test_for_each_async_preserves_order() {
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let collected = Arc::new(Mutex::new(Vec::new()));
		let collected_clone = collected.clone();

		stream
			.for_each_async(move |coord, val| {
				let c = collected_clone.clone();
				async move {
					c.lock().unwrap().push((coord.x, val));
				}
			})
			.await;

		let items = collected.lock().unwrap();
		for (i, (x, val)) in items.iter().enumerate() {
			let expected = u32::try_from(i).unwrap();
			assert_eq!(*x, expected);
			assert_eq!(*val, expected);
		}
	}

	// -------------------------------------------------------------------------
	// for_each_async_try (async, sequential, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_async_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let result = stream.for_each_async_try(|_coord, _blob| async { Ok(()) }).await;

		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_for_each_async_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let result = stream
			.for_each_async_try(|_coord, _blob| async { bail!("test error") })
			.await;

		assert!(result.is_err());
	}

	// -------------------------------------------------------------------------
	// for_each_parallel_async (async, parallel)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_parallel_async() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let counter = Arc::new(AtomicUsize::new(0));
		let counter_clone = counter.clone();

		stream
			.for_each_parallel_async(move |_coord, _blob| {
				let c = counter_clone.clone();
				async move {
					tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
					c.fetch_add(1, Ordering::Relaxed);
				}
			})
			.await;

		assert_eq!(counter.load(Ordering::Relaxed), 2);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_for_each_parallel_async_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());

		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		stream
			.for_each_parallel_async(move |_coord, _item| {
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
			.await;

		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
	}

	// -------------------------------------------------------------------------
	// for_each_parallel_async_try (async, parallel, fallible)
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_parallel_async_try_success() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let result = stream
			.for_each_parallel_async_try(|_coord, _blob| async { Ok(()) })
			.await;

		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_for_each_parallel_async_try_error() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("data"))]);

		let result = stream
			.for_each_parallel_async_try(|_coord, _blob| async { bail!("test error") })
			.await;

		assert!(result.is_err());
	}
}
