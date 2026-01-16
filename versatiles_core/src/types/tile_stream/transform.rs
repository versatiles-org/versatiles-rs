use super::{Arc, ConcurrencyLimits, Future, Result, StreamExt, TileCoord, TileStream, ready, stream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Parallel Transformations
	// -------------------------------------------------------------------------

	/// Transforms the **value of type `T`** for each tile in parallel using the provided closure `callback`.
	///
	/// Spawns tokio tasks with CPU-bound concurrency limit. Each item `(coord, value)` is mapped
	/// to `(coord, Result<callback(value)>)`.
	///
	/// Uses CPU-bound concurrency limit since the callback runs in `spawn_blocking`.
	///
	/// Returns a stream of `Result<O>` values. If the callback returns an error for any tile,
	/// that error is propagated as an item in the stream. Consumers can use `.try_for_each()`,
	/// `.try_collect()`, or similar methods to fail fast on the first error, or handle errors
	/// individually.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # use futures::{StreamExt, TryStreamExt};
	/// # async fn test() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped = stream.map_item_parallel(|value| {
	///     // Example transformation on the tile value
	///     Ok(Blob::from(format!("mapped {}", value.as_str())))
	/// });
	///
	/// // Collect results, failing fast on first error
	/// let items: Vec<(TileCoord, Blob)> = mapped
	///     .inner
	///     .filter_map(|(coord, result)| async move {
	///         match result {
	///             Ok(item) => Some(Ok((coord, item))),
	///             Err(e) => Some(Err(e)),
	///         }
	///     })
	///     .try_collect()
	///     .await?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn map_item_parallel<F, O>(self, callback: F) -> TileStream<'a, Result<O>>
	where
		F: Fn(T) -> Result<O> + Send + Sync + 'static,
		T: 'static,
		O: Send + Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(item)))
			})
			.buffer_unordered(limits.cpu_bound) // CPU-bound: spawn_blocking
			.map(|result| match result {
				Ok((coord, Ok(item))) => (coord, Ok(item)),
				Ok((coord, Err(e))) => (coord, Err(e.context(format!("Failed to process tile at {coord:?}")))),
				Err(e) => panic!("Spawned task panicked: {e}"), // Task panic is still a panic (unexpected)
			});
		TileStream { inner: s.boxed() }
	}

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
	/// let subdivided = stream.flat_map_parallel(|coord, blob| {
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
	pub fn flat_map_parallel<F, O>(self, callback: F) -> TileStream<'a, Result<O>>
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

	/// Filters and transforms the **value of type `T`** for each tile in parallel, discarding items where `callback` returns `None`.
	///
	/// Spawns tokio tasks with CPU-bound concurrency limit. Each item `(coord, value)` is mapped
	/// to `(coord, Result<callback(value)>)`. If `callback` returns `Ok(None)`, the item is dropped.
	/// If it returns `Ok(Some(value))`, the item is kept. If it returns `Err`, the error is propagated.
	///
	/// Returns a stream of `Result<O>` values. If the callback returns an error for any tile,
	/// that error is propagated as an item in the stream. Items where the callback returns `Ok(None)`
	/// are filtered out.
	///
	/// Uses CPU-bound concurrency limit since the callback runs in `spawn_blocking`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use anyhow::Result;
	/// # use futures::{StreamExt, TryStreamExt};
	/// # async fn test() -> Result<()> {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("keep")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_item_parallel(|value| {
	///     Ok(if value.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("was: {}", value.as_str())))
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
	pub fn filter_map_item_parallel<F, O>(self, callback: F) -> TileStream<'a, Result<O>>
	where
		F: Fn(T) -> Result<Option<O>> + Send + Sync + 'static,
		T: 'static,
		O: Send + Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(item)))
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

	// -------------------------------------------------------------------------
	// Coordinate Transformations
	// -------------------------------------------------------------------------

	/// Applies a synchronous coordinate transformation to each `(TileCoord, Blob)` item.
	///
	/// Maintains the same value of type `T`, but transforms `coord` via `callback`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped_coords = stream.map_coord(|coord| {
	///     TileCoord::new(coord.level + 1, coord.x, coord.y).unwrap()
	/// });
	///
	/// let items = mapped_coords.to_vec().await;
	/// // The tile data remains the same, but each coordinate has its level incremented.
	/// # }
	/// ```
	pub fn map_coord<F>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord) -> TileCoord + Send + 'a,
	{
		let s = self.inner.map(move |(coord, item)| (callback(coord), item)).boxed();
		TileStream { inner: s }
	}

	/// Filters the stream by **tile coordinate** using an *asynchronous* predicate.
	///
	/// The provided closure receives each `TileCoord` and returns a `Future<bool>`.
	/// If the future resolves to `true`, the item is kept; otherwise it is dropped.
	///
	/// This is analogous to [`StreamExt::filter`] but operates on the coordinate
	/// only, leaving the associated value of type `T` untouched.
	///
	/// # Arguments
	/// * `callback` – async predicate `Fn(TileCoord) -> Future<Output = bool>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn demo() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(5,5,5).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// // Keep only tiles at zoom level 0.
	/// let filtered = stream.filter_coord(|coord| async move { coord.level == 0 });
	/// let items = filtered.to_vec().await;
	///
	/// assert_eq!(items.len(), 1);
	/// assert_eq!(items[0].0.level, 0);
	/// # }
	/// ```
	pub fn filter_coord<F, Fut>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord) -> Fut + Send + 'a,
		Fut: Future<Output = bool> + Send + 'a,
	{
		let s = self.inner.filter(move |(coord, _item)| callback(*coord)).boxed();
		TileStream { inner: s }
	}

	/// Observes each item passing through the stream by calling a callback.
	///
	/// This method is useful for side effects like progress tracking, logging, or metrics collection.
	/// The callback is invoked once per item but receives no arguments and cannot modify the items.
	/// The stream passes through unchanged.
	///
	/// # Use Cases
	///
	/// - **Progress tracking**: Increment a counter to show processing progress
	/// - **Logging**: Record that an item was processed without inspecting it
	/// - **Metrics**: Count the total number of items in a stream
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
	///     .inspect(move || {
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
		F: FnMut() + Send + 'a,
	{
		TileStream {
			inner: self
				.inner
				.map(move |item| {
					callback();
					item
				})
				.boxed(),
		}
	}

	// -------------------------------------------------------------------------
	// Utility
	// -------------------------------------------------------------------------

	/// Drains this stream of all items, returning the total count of processed items.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let count = stream.drain_and_count().await;
	/// assert_eq!(count, 2);
	/// # }
	/// ```
	pub async fn drain_and_count(self) -> u64 {
		let mut count = 0u64;
		self
			.inner
			.for_each(|_| {
				count += 1;
				ready(())
			})
			.await;
		count
	}
}

/// Methods specific to `TileStream<'a, Result<T>>`
impl<'a, T> TileStream<'a, Result<T>>
where
	T: Send + 'a,
{
	/// Unwraps Results from a `TileStream<'a, Result<T>>`, panicking on errors.
	///
	/// This is a convenience method for backward compatibility with code that doesn't need
	/// fine-grained error handling. If any item in the stream is an `Err`, this will panic
	/// with the error message.
	///
	/// For proper error handling, use the stream's `.inner` field directly with `try_collect()`,
	/// `try_for_each()`, or similar methods from `TryStreamExt`.
	///
	/// # Panics
	/// Panics if any item in the stream is an `Err`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data")),
	/// ]);
	///
	/// // Process with parallel operation, then unwrap results
	/// let processed = stream
	///     .map_item_parallel(|blob| Ok(Blob::from(format!("processed-{}", blob.as_str()))))
	///     .unwrap_results();
	///
	/// // Now `processed` is TileStream<'a, Blob>, not TileStream<'a, Result<Blob>>
	/// # }
	/// ```
	pub fn unwrap_results(self) -> TileStream<'a, T> {
		TileStream {
			inner: self
				.inner
				.map(|(coord, result)| {
					let item = result.unwrap_or_else(|e| panic!("Stream contained error at {coord:?}: {e}"));
					(coord, item)
				})
				.boxed(),
		}
	}
}
