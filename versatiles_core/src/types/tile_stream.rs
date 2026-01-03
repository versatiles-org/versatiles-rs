//! Asynchronous tile stream processing
//!
//! This module provides [`TileStream`], an asynchronous stream abstraction for processing
//! map tiles in parallel. Each tile is represented by a coordinate ([`TileCoord`]) and an
//! associated value of generic type `T` (default: [`Blob`]).
//!
//! # Features
//!
//! - **Parallel Processing**: Transform or filter tile data in parallel using tokio tasks
//! - **Buffering**: Collect or process data in configurable batches
//! - **Flexible Callbacks**: Choose between sync and async processing steps
//! - **Stream Composition**: Flatten and combine multiple tile streams
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::{TileStream, TileCoord, Blob};
//!
//! # async fn example() {
//! // Create a stream from coordinates
//! let coords = vec![
//!     TileCoord::new(5, 10, 15).unwrap(),
//!     TileCoord::new(5, 11, 15).unwrap(),
//! ];
//!
//! let stream = TileStream::from_vec(
//!     coords.into_iter()
//!         .map(|coord| (coord, Blob::from("tile data")))
//!         .collect()
//! );
//!
//! // Process tiles asynchronously
//! stream.for_each_async(|(coord, blob)| async move {
//!     println!("Processing tile {:?}, size: {}", coord, blob.len());
//! }).await;
//! # }
//! ```
use crate::{Blob, ConcurrencyLimits, TileCoord};
use anyhow::Result;
use futures::{
	Future, Stream, StreamExt, TryStreamExt,
	future::ready,
	stream::{self, BoxStream},
};
use std::{collections::HashMap, pin::Pin, sync::Arc};

/// A stream of tiles represented by `(TileCoord, T)` pairs.
///
/// # Type Parameters
/// - `'a`: The lifetime of the stream.
/// - `T`: The type of the tile data, defaulting to `Blob`.
///
/// # Fields
/// - `stream`: The internal boxed stream that emits `(TileCoord, T)` pairs.
pub struct TileStream<'a, T = Blob> {
	/// The internal boxed stream, emitting `(TileCoord, T)` pairs.
	pub inner: BoxStream<'a, (TileCoord, T)>,
}

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Constructors
	// -------------------------------------------------------------------------

	/// Creates a `TileStream` containing no items.
	///
	/// Useful for representing an empty data source.
	#[must_use]
	pub fn empty() -> TileStream<'a, T> {
		TileStream {
			inner: stream::empty().boxed(),
		}
	}

	/// Creates a `TileStream` from an existing `Stream` of `(TileCoord, T)`.
	///
	/// # Examples
	/// ```
	/// use futures::{stream, StreamExt};
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	///
	/// let tile_data = stream::iter(vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("tile1")),
	/// ]);
	/// let my_stream = TileStream::from_stream(tile_data.boxed());
	/// ```
	#[must_use]
	pub fn from_stream(stream: Pin<Box<dyn Stream<Item = (TileCoord, T)> + Send + 'a>>) -> Self {
		TileStream { inner: stream }
	}

	/// Constructs a `TileStream` from a vector of `(TileCoord, T)` items.
	///
	/// The resulting stream will yield each item in `vec` in order.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// let tile_data = vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
	///     (TileCoord::new(1, 1, 1).unwrap(), Blob::from("tile1")),
	/// ];
	/// let tile_stream = TileStream::from_vec(tile_data);
	/// ```
	#[must_use]
	pub fn from_vec(vec: Vec<(TileCoord, T)>) -> Self {
		TileStream {
			inner: stream::iter(vec).boxed(),
		}
	}

	// -------------------------------------------------------------------------
	// Stream Creation from Iterators
	// -------------------------------------------------------------------------

	/// Creates a `TileStream` by converting an iterator of `TileCoord` into parallel tasks
	/// that produce `(TileCoord, T)` items asynchronously.
	///
	/// Spawns one tokio task per coordinate (buffered by CPU-bound concurrency limit), calling `callback`
	/// to produce the tile value. Returns only items where `callback(coord)` yields `Some(value)`.
	///
	/// Uses CPU-bound concurrency limit since the callback runs in `spawn_blocking`.
	///
	/// # Arguments
	/// * `iter` - An iterator of tile coordinates.
	/// * `callback` - A shared closure returning `Option<Blob>` for each coordinate.
	///
	/// # Examples
	/// ```
	/// # use std::sync::Arc;
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// let coords = vec![TileCoord::new(0,0,0).unwrap(), TileCoord::new(1,1,1).unwrap()];
	/// let closure = |coord: TileCoord| {
	///     // Data loading logic...
	///     Some(Blob::from(format!("data for {:?}", coord)))
	/// };
	///
	/// let tile_stream = TileStream::from_iter_coord_parallel(coords.into_iter(), closure);
	/// ```
	pub fn from_iter_coord_parallel<F>(iter: impl Iterator<Item = TileCoord> + Send + 'a, callback: F) -> Self
	where
		F: Fn(TileCoord) -> Option<T> + Send + Sync + 'static,
		T: 'static,
	{
		let callback = Arc::new(callback);
		let limits = ConcurrencyLimits::default();
		let s = stream::iter(iter)
			.map(move |coord| {
				let cb = Arc::clone(&callback);
				// Spawn a task for each coordinate
				tokio::task::spawn_blocking(move || (coord, cb(coord)))
			})
			.buffer_unordered(limits.cpu_bound) // CPU-bound: spawn_blocking
			.filter_map(|result| async {
				match result {
					Ok((coord, Some(item))) => Some((coord, item)),
					_ => None,
				}
			});
		TileStream { inner: s.boxed() }
	}

	/// Creates a `TileStream` by sequentially filtering and mapping coordinates from an iterator.
	///
	/// For each coordinate in `iter`, calls `callback(coord)`. If the callback returns `Some(item)`,
	/// the `(coord, item)` pair is included in the stream. If it returns `None`, the coordinate
	/// is skipped.
	///
	/// This is the **sequential** version that processes coordinates one at a time without parallelism.
	/// For CPU-intensive callbacks or large iterators, consider using [`TileStream::from_iter_coord_parallel`].
	///
	/// # When to Use
	///
	/// - When the callback is very fast (e.g., simple filtering or lookups)
	/// - When you want deterministic ordering (parallel version uses unordered processing)
	/// - When the overhead of spawning tasks exceeds the benefit of parallelism
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::{TileCoord, TileStream, Blob};
	///
	/// # async fn example() {
	/// let coords = vec![
	///     TileCoord::new(0, 0, 0).unwrap(),
	///     TileCoord::new(1, 0, 0).unwrap(),
	///     TileCoord::new(2, 0, 0).unwrap(),
	/// ];
	///
	/// // Only include even zoom levels
	/// let stream = TileStream::from_iter_coord(coords.into_iter(), |coord| {
	///     if coord.level % 2 == 0 {
	///         Some(Blob::from(format!("level {}", coord.level)))
	///     } else {
	///         None
	///     }
	/// });
	///
	/// let items = stream.to_vec().await;
	/// assert_eq!(items.len(), 2); // levels 0 and 2
	/// # }
	/// ```
	pub fn from_iter_coord<F>(iter: impl Iterator<Item = TileCoord> + Send + 'a, callback: F) -> Self
	where
		F: Fn(TileCoord) -> Option<T> + Send + Sync + 'static,
		T: 'static,
	{
		TileStream {
			inner: stream::iter(iter.filter_map(move |coord| callback(coord).map(|item| (coord, item)))).boxed(),
		}
	}

	/// Creates a `TileStream` by filtering and mapping an async closure over a vector of tile coordinates.
	///
	/// The closure `callback` takes a coordinate and returns a `Future` that yields
	/// an `Option<(TileCoord, T)>`. Only `Some` items are emitted.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use futures::Future;
	/// # async fn example() {
	/// let coords = vec![TileCoord::new(0,0,0).unwrap(), TileCoord::new(1,1,1).unwrap()];
	/// let closure = |coord: TileCoord| async move {
	///     if coord.level == 0 {
	///         Some((coord, Blob::from("data")))
	///     } else {
	///         None
	///     }
	/// };
	///
	/// let tile_stream = TileStream::from_coord_vec_async(coords, closure);
	/// let items = tile_stream.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn from_coord_vec_async<F, Fut>(vec: Vec<TileCoord>, callback: F) -> Self
	where
		F: FnMut(TileCoord) -> Fut + Send + 'a,
		Fut: Future<Output = Option<(TileCoord, T)>> + Send + 'a,
	{
		let s = stream::iter(vec).filter_map(callback);
		TileStream { inner: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Stream Flattening
	// -------------------------------------------------------------------------

	/// Flattens multiple `TileStream`s from an iterator of `Future`s into a single `TileStream`.
	///
	/// This method awaits each future to obtain a `TileStream`, then flattens all items into one stream.
	///
	/// # Arguments
	/// * `iter` - An iterator of futures that yield `TileStream`s.
	/// * `cores_per_task` - The number of CPU cores to allocate for each task.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use futures::{future, stream};
	/// #
	/// async fn example(tile_streams: Vec<impl std::future::Future<Output=TileStream<'static>> + Send + 'static>) {
	///     let merged = TileStream::from_streams(stream::iter(tile_streams));
	///     let all_items = merged.to_vec().await;
	///     // `all_items` now contains items from all child streams
	/// }
	/// ```
	pub fn from_streams<FutureStream>(streams: impl Stream<Item = FutureStream> + Send + 'a) -> TileStream<'a, T>
	where
		FutureStream: Future<Output = TileStream<'a, T>> + Send + 'a,
	{
		let limits = ConcurrencyLimits::default();
		TileStream {
			inner: Box::pin(streams.buffer_unordered(limits.io_bound).map(|s| s.inner).flatten()), // I/O-bound: awaiting async streams
		}
	}

	// -------------------------------------------------------------------------
	// Collecting and Iteration
	// -------------------------------------------------------------------------

	/// Collects all `(TileCoord, T)` items from this stream into a vector.
	///
	/// Consumes the stream.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	/// let items = stream.to_vec().await;
	/// assert_eq!(items.len(), 2);
	/// # }
	/// ```
	pub async fn to_vec(self) -> Vec<(TileCoord, T)> {
		self.inner.collect().await
	}

	/// Collects all items from the stream into a [`HashMap`] keyed by coordinate.
	///
	/// This consumes the stream and returns a map that allows O(1) random access to tiles by their
	/// coordinates. Useful when you need to look up tiles by coordinate frequently, or when you need
	/// to check if a specific tile exists.
	///
	/// **Note**: If the stream contains duplicate coordinates, only the **last** value for each
	/// coordinate will be retained in the map.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::{TileCoord, TileStream, Blob};
	/// use std::collections::HashMap;
	///
	/// # async fn example() {
	/// let items = vec![
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
	///     (TileCoord::new(1, 0, 0).unwrap(), Blob::from("tile1")),
	/// ];
	///
	/// let stream = TileStream::from_vec(items);
	/// let map: HashMap<TileCoord, Blob> = stream.to_map().await;
	///
	/// // Fast lookup by coordinate
	/// let coord = TileCoord::new(1, 0, 0).unwrap();
	/// assert!(map.contains_key(&coord));
	/// assert_eq!(map.get(&coord).unwrap().as_str(), "tile1");
	/// # }
	/// ```
	pub async fn to_map(self) -> HashMap<TileCoord, T> {
		self.inner.collect().await
	}

	/// Retrieves the next `(TileCoord, T)` item from this stream, or `None` if the stream is empty.
	///
	/// The internal pointer advances by one item.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn test() {
	/// let mut stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let first = stream.next().await;
	/// assert!(first.is_some());
	/// let second = stream.next().await;
	/// assert!(second.is_some());
	/// let third = stream.next().await;
	/// assert!(third.is_none());
	/// # }
	/// ```
	pub async fn next(&mut self) -> Option<(TileCoord, T)> {
		self.inner.next().await
	}

	/// Applies an asynchronous callback `callback` to each `(TileCoord, T)` item.
	///
	/// Consumes the stream. The provided closure returns a `Future<Output=()>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # use futures::Future;
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_async(|(coord, value)| async move {
	///     println!("coord={:?}, value={:?}", coord, value);
	/// }).await;
	/// # }
	/// ```
	pub async fn for_each_async<F, Fut>(self, callback: F)
	where
		F: FnMut((TileCoord, T)) -> Fut,
		Fut: Future<Output = ()>,
	{
		self.inner.for_each(callback).await;
	}

	/// Applies an async callback to each item in parallel with concurrency limits.
	///
	/// Unlike [`for_each_async`](Self::for_each_async) which processes items sequentially, this method
	/// processes multiple items concurrently up to a concurrency limit. This is ideal for I/O-bound
	/// operations like writing tiles to disk, uploading to remote storage, or making network requests.
	///
	/// The concurrency limit is set to `ConcurrencyLimits::default().mixed`, which balances between
	/// CPU and I/O workloads.
	///
	/// # When to Use
	///
	/// - **I/O-bound operations**: Writing files, network requests, database operations
	/// - **Mixed workloads**: Operations that involve both computation and I/O
	/// - **When order doesn't matter**: Items may complete in any order
	///
	/// For sequential processing where order matters, use [`for_each_async`](Self::for_each_async).
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
	///     (TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
	///     (TileCoord::new(1, 0, 0).unwrap(), Blob::from("tile1")),
	///     (TileCoord::new(2, 0, 0).unwrap(), Blob::from("tile2")),
	/// ]);
	///
	/// // Process tiles in parallel (e.g., simulating I/O operations)
	/// stream.for_each_async_parallel(move |(coord, _blob)| {
	///     let c = Arc::clone(&counter_clone);
	///     async move {
	///         // Simulate async I/O work
	///         tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
	///         c.fetch_add(1, Ordering::Relaxed);
	///     }
	/// }).await;
	///
	/// assert_eq!(counter.load(Ordering::Relaxed), 3); // 3 tiles processed
	/// # }
	/// ```
	pub async fn for_each_async_parallel<F, Fut>(self, callback: F)
	where
		F: FnMut((TileCoord, T)) -> Fut,
		Fut: Future<Output = ()>,
	{
		let limits = ConcurrencyLimits::default();
		self.inner.for_each_concurrent(limits.mixed, callback).await; // Mixed: async callback (I/O + CPU)
	}

	/// Applies a synchronous callback `callback` to each `(TileCoord, T)` item.
	///
	/// Consumes the stream. The provided closure returns `()`.
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
	/// stream.for_each_sync(|(coord, value)| {
	///     println!("coord={:?}, value={:?}", coord, value);
	/// }).await;
	/// # }
	/// ```
	pub async fn for_each_sync<F>(self, mut callback: F)
	where
		F: FnMut((TileCoord, T)),
	{
		self
			.inner
			.for_each(|item| {
				callback(item);
				ready(())
			})
			.await;
	}

	/// Buffers items in chunks of size `buffer_size`, then calls `callback` with each full or final chunk.
	///
	/// Consumes the stream. Items are emitted in `(TileCoord, T)` form.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord::new(1,1,1).unwrap(), Blob::from("data1")),
	///     (TileCoord::new(2,2,2).unwrap(), Blob::from("data2")),
	/// ]);
	///
	/// stream.for_each_buffered(2, |chunk| {
	///     println!("Processing chunk of size: {}", chunk.len());
	/// }).await;
	/// // Output:
	/// // "Processing chunk of size: 2"
	/// // "Processing chunk of size: 1"
	/// # }
	/// ```
	pub async fn for_each_buffered<F>(mut self, buffer_size: usize, mut callback: F)
	where
		F: FnMut(Vec<(TileCoord, T)>),
	{
		let mut buffer = Vec::with_capacity(buffer_size);
		while let Some(item) = self.inner.next().await {
			buffer.push(item);

			if buffer.len() >= buffer_size {
				callback(buffer);
				buffer = Vec::with_capacity(buffer_size);
			}
		}
		if !buffer.is_empty() {
			callback(buffer);
		}
	}

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
	/// * `callback` – async predicate `Fn(TileCoord) -> Future<Output = bool>`.
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

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use tokio::sync::Mutex;

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	#[tokio::test]
	async fn should_flat_map_parallel_and_flatten_results() {
		// Base stream with two coords
		let base = TileStream::from_vec(vec![(tc(1, 0, 0), 10u32), (tc(1, 1, 0), 20u32)]);

		// Each item expands to a sub-stream with two entries
		let flat = base.flat_map_parallel(|coord, val| {
			let out = vec![
				(coord, format!("a:{val}")),
				(tc(coord.level, coord.x, coord.y + 1), format!("b:{val}")),
			];
			Ok(TileStream::from_vec(out))
		});

		// Unwrap Results
		let mut items: Vec<(TileCoord, String)> = flat
			.inner
			.map(|(coord, result)| result.map(|item| (coord, item)))
			.try_collect()
			.await
			.unwrap();

		// Sort for deterministic assertions
		items.sort_by_key(|(c, b)| (c.x, c.y, b.as_str().to_string()));

		assert_eq!(
			items,
			[
				(tc(1, 0, 0), "a:10".into()),
				(tc(1, 0, 1), "b:10".into()),
				(tc(1, 1, 0), "a:20".into()),
				(tc(1, 1, 1), "b:20".into()),
			]
		);
	}

	#[tokio::test]
	async fn should_collect_all_items_from_vec() {
		let tile_data = vec![(tc(0, 0, 0), Blob::from("tile0")), (tc(1, 1, 1), Blob::from("tile1"))];

		let tile_stream = TileStream::from_vec(tile_data.clone());
		let collected = tile_stream.to_vec().await;

		assert_eq!(collected, tile_data);
	}

	#[tokio::test]
	async fn should_iterate_sync_over_items() {
		let tile_data = vec![
			(tc(0, 0, 0), Blob::from("tile0")),
			(tc(1, 1, 1), Blob::from("tile1")),
			(tc(2, 2, 2), Blob::from("tile2")),
		];

		let tile_stream = TileStream::from_vec(tile_data);

		let mut result = vec![];
		tile_stream
			.for_each_sync(|(coord, blob)| {
				result.push(format!("{}, {}", coord.as_json(), blob.as_str()));
			})
			.await;

		assert_eq!(
			result,
			[
				"{\"z\":0,\"x\":0,\"y\":0}, tile0",
				"{\"z\":1,\"x\":1,\"y\":1}, tile1",
				"{\"z\":2,\"x\":2,\"y\":2}, tile2"
			]
		);
	}

	#[tokio::test]
	async fn should_map_coord_properly() {
		let original = TileStream::from_vec(vec![(tc(3, 1, 2), Blob::from("data"))]);

		let mapped = original.map_coord(|coord| tc(coord.level + 1, coord.x * 2, coord.y * 2));

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 1);
		let (coord, blob) = &items[0];
		assert_eq!(coord.x, 2);
		assert_eq!(coord.y, 4);
		assert_eq!(coord.level, 4);
		assert_eq!(blob.as_str(), "data");
	}

	#[tokio::test]
	async fn should_count_items_with_drain_and_count() {
		let tile_data = vec![
			(tc(0, 0, 0), Blob::from("tile0")),
			(tc(1, 1, 1), Blob::from("tile1")),
			(tc(2, 2, 2), Blob::from("tile2")),
		];

		let tile_stream = TileStream::from_vec(tile_data);
		let count = tile_stream.drain_and_count().await;
		assert_eq!(count, 3, "Should drain exactly 3 items");
	}

	#[tokio::test]
	async fn should_run_for_each_buffered_in_chunks() {
		let tile_data = vec![
			(tc(0, 0, 0), Blob::from("tile0")),
			(tc(1, 1, 1), Blob::from("tile1")),
			(tc(2, 2, 2), Blob::from("tile2")),
		];

		let tile_stream = TileStream::from_vec(tile_data);
		let mut results = Vec::new();

		tile_stream
			.for_each_buffered(2, |chunk| {
				// Each chunk is at most size 2
				results.push(chunk.len());
			})
			.await;

		// Should process a chunk of size 2, then a chunk of size 1
		assert_eq!(results, vec![2, 1]);
	}

	#[tokio::test]
	async fn should_do_parallel_blob_mapping() {
		let tile_data = vec![(tc(0, 0, 0), Blob::from("zero")), (tc(1, 1, 1), Blob::from("one"))];

		// Apply parallel mapping
		let transformed = TileStream::from_vec(tile_data.clone())
			.map_item_parallel(|blob| Ok(Blob::from(format!("mapped-{}", blob.as_str()))));

		// Collect results, unwrapping the Results
		let mut items: Vec<(TileCoord, Blob)> = transformed
			.inner
			.map(|(coord, result)| result.map(|item| (coord, item)))
			.try_collect()
			.await
			.unwrap();
		assert_eq!(items.len(), 2, "Expected two items after mapping");

		// Sort by coordinate level to allow for unordered execution
		items.sort_by_key(|(coord, _)| coord.level);

		// Verify that coordinates are preserved and blobs correctly mapped
		assert_eq!(items[0].0, tc(0, 0, 0));
		assert_eq!(items[0].1.as_str(), "mapped-zero");
		assert_eq!(items[1].0, tc(1, 1, 1));
		assert_eq!(items[1].1.as_str(), "mapped-one");
	}

	#[tokio::test]
	async fn should_parallel_filter_map_blob_correctly() {
		let tile_data = vec![
			(tc(0, 0, 0), Blob::from("keep0")),
			(tc(1, 1, 1), Blob::from("discard1")),
			(tc(2, 2, 2), Blob::from("keep2")),
		];

		let filtered = TileStream::from_vec(tile_data).filter_map_item_parallel(|blob| {
			Ok(if blob.as_str().starts_with("discard") {
				None
			} else {
				Some(Blob::from(format!("kept-{}", blob.as_str())))
			})
		});

		// Collect results, unwrapping the Results
		let items: Vec<(TileCoord, Blob)> = filtered
			.inner
			.map(|(coord, result)| result.map(|item| (coord, item)))
			.try_collect()
			.await
			.unwrap();
		let mut texts = items.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>();
		texts.sort_unstable();
		assert_eq!(texts, ["kept-keep0", "kept-keep2"]);
	}

	#[tokio::test]
	async fn should_construct_empty_stream() {
		let empty = TileStream::<Blob>::empty();
		let collected = empty.to_vec().await;
		assert!(collected.is_empty());
	}

	#[tokio::test]
	async fn should_construct_from_iter_stream() {
		// Create multiple sub-streams
		let substreams = vec![
			Box::pin(async { TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("sub0-0"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
			Box::pin(async { TileStream::from_vec(vec![(tc(1, 1, 1), Blob::from("sub1-1"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
		];

		// Merge them
		let merged = TileStream::<Blob>::from_streams(stream::iter(substreams));
		let items = merged.to_vec().await;
		assert_eq!(items.len(), 2);
	}

	#[tokio::test]
	async fn should_return_none_if_stream_is_empty() {
		let mut empty = TileStream::<Blob>::empty();
		assert!(empty.next().await.is_none());
	}

	#[tokio::test]
	async fn should_process_async_for_each() {
		let tile_data = vec![(tc(0, 0, 0), Blob::from("async0")), (tc(1, 1, 1), Blob::from("async1"))];

		let s = TileStream::from_vec(tile_data);
		let collected_mutex = Arc::new(Mutex::new(Vec::new()));

		let collected_clone = Arc::clone(&collected_mutex);
		s.for_each_async(move |(coord, blob)| {
			let collected = Arc::clone(&collected_clone);
			async move {
				collected.lock().await.push((coord, blob));
			}
		})
		.await;

		let collected = collected_mutex.lock().await;
		assert_eq!(collected.len(), 2);
		assert_eq!(collected[0].1.as_str(), "async0");
		assert_eq!(collected[1].1.as_str(), "async1");
	}

	#[tokio::test]
	async fn should_filter_by_coord() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("z0")), (tc(1, 1, 1), Blob::from("z1"))]);

		let filtered = stream.filter_coord(|coord| async move { coord.level == 0 });
		let items = filtered.to_vec().await;

		assert_eq!(items.len(), 1);
		assert_eq!(items[0].0.level, 0);
		assert_eq!(items[0].1.as_str(), "z0");
	}

	#[tokio::test]
	async fn should_create_from_iter_coord_parallel() {
		let coords = vec![tc(0, 0, 0), tc(1, 1, 1)];

		let stream = TileStream::from_iter_coord_parallel(coords.into_iter(), |coord| {
			Some(Blob::from(format!("v{}", coord.level)))
		});

		let mut items = stream.to_vec().await;
		// Sort for deterministic assertion on unordered parallel output
		items.sort_by_key(|(coord, _)| coord.level);

		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "v0");
		assert_eq!(items[1].1.as_str(), "v1");
	}

	#[tokio::test]
	async fn should_create_from_coord_vec_async() {
		let coords = vec![tc(0, 0, 0), tc(1, 1, 1)];

		let stream = TileStream::from_coord_vec_async(coords, |coord| async move {
			if coord.level == 0 {
				Some((coord, Blob::from("keep")))
			} else {
				None
			}
		});

		let items = stream.to_vec().await;
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].0.level, 0);
		assert_eq!(items[0].1.as_str(), "keep");
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_map_item_parallel_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());
		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		let stream = stream.map_item_parallel(move |item| {
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
			Ok(item)
		});

		// Collect results, unwrapping the Results
		let results: Vec<(TileCoord, u32)> = stream
			.inner
			.map(|(coord, result)| result.map(|item| (coord, item)))
			.try_collect()
			.await
			.unwrap();
		assert_eq!(results.len(), 6);
		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_filter_map_item_parallel_parallelism() {
		let stream = TileStream::from_vec(
			vec![Some(1), None, Some(3), None, Some(5), None]
				.into_iter()
				.enumerate()
				.map(|(i, v)| (tc(12, i as u32, 0), v))
				.collect::<Vec<_>>(),
		);
		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		let stream = stream.filter_map_item_parallel(move |item| {
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
			Ok(item)
		});

		// Collect results, unwrapping the Results
		let results: Vec<(TileCoord, u32)> = stream
			.inner
			.map(|(coord, result)| result.map(|item| (coord, item)))
			.try_collect()
			.await
			.unwrap();
		assert_eq!(results.len(), 3);
		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_for_each_async_parallel_parallelism() {
		let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());
		let counter = Arc::new(AtomicUsize::new(0));
		let max_parallel = Arc::new(AtomicUsize::new(0));
		let current_parallel = Arc::new(AtomicUsize::new(0));

		let counter_clone = counter.clone();
		let max_parallel_clone = max_parallel.clone();
		let current_parallel_clone = current_parallel.clone();

		stream
			.for_each_async_parallel(move |_item| {
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
					tokio::time::sleep(std::time::Duration::from_millis(10)).await;
					current_parallel.fetch_sub(1, Ordering::SeqCst);
					counter.fetch_add(1, Ordering::SeqCst);
				}
			})
			.await;

		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1);
	}

	#[tokio::test]
	async fn should_merge_streams_with_large_cores_per_task() {
		// cores_per_task larger than CPU count should still work (limit clamped to 1)
		let substreams = vec![
			Box::pin(async { TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
			Box::pin(async { TileStream::from_vec(vec![(tc(1, 1, 1), Blob::from("b"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
		];
		let merged = TileStream::<Blob>::from_streams(stream::iter(substreams));
		let items = merged.to_vec().await;
		assert_eq!(items.len(), 2);
	}
}
