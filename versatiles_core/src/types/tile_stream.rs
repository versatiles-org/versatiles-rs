/// A module defining the `TileStream` struct, which provides asynchronous handling of a stream of tiles.
///
/// Each tile is represented by a coordinate (`TileCoord`) and an associated value of **generic type `T`** (default: `Blob`). The `TileStream`
/// offers methods for parallel processing, buffering, synchronization callbacks, and easy iteration.
///
/// # Features
/// - **Parallel Processing**: Transform or filter tile data in parallel using tokio tasks.
/// - **Buffering**: Collect or process data in configurable batches.
/// - **Synchronous and Asynchronous Callbacks**: Choose between sync and async processing steps.
///
/// # Structs
/// - `TileStream`: Encapsulates a stream of `(TileCoord, T)` tuples, providing methods for transformation, iteration, and buffering.
///
/// # Methods
/// ## Constructors
/// - `new_empty`: Creates an empty `TileStream`.
/// - `from_stream`: Constructs a `TileStream` from an existing `Stream`.
/// - `from_vec`: Constructs a `TileStream` from a vector of `(TileCoord, T)` items.
/// - `from_iter_coord_parallel`: Creates a `TileStream` from an iterator of coordinates, processing them in parallel.
/// - `from_coord_vec_async`: Creates a `TileStream` from a vector of coordinates, applying an async closure.
///
/// ## Stream Flattening
/// - `from_iter_stream`: Flattens multiple `TileStream`s from an iterator of `Future`s into a single `TileStream`.
///
/// ## Collecting and Iteration
/// - `collect`: Collects all items from the stream into a vector.
/// - `next`: Retrieves the next item from the stream.
/// - `for_each_async`: Applies an async callback to each item.
/// - `for_each_sync`: Applies a sync callback to each item.
/// - `for_each_buffered`: Buffers items in chunks and processes them.
///
/// ## Parallel Transformations
/// - `map_blob_parallel`: Transforms the value of type `T` for each tile in parallel.
/// - `filter_map_blob_parallel`: Filters and transforms the value of type `T` for each tile in parallel.
///
/// ## Coordinate Transformations
/// - `map_coord`: Applies a synchronous coordinate transformation to each item.
///
/// ## Utility
/// - `drain_and_count`: Drains the stream and returns the total count of items.
///
/// # Utility Functions
/// - `unwrap_result`: Unwraps a `Result`, printing detailed error information and terminating the program on failure.
use crate::{Blob, TileCoord};
use anyhow::Result;
use futures::{
	Future, Stream, StreamExt,
	future::ready,
	stream::{self, BoxStream},
};
use std::{collections::HashMap, io::Write, pin::Pin, sync::Arc};

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
	/// Spawns one tokio task per coordinate (buffered by `num_cpus::get()`), calling `callback`
	/// to produce the tile value. Returns only items where `callback(coord)` yields `Some(value)`.
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
		let s = stream::iter(iter)
			.map(move |coord| {
				let cb = Arc::clone(&callback);
				// Spawn a task for each coordinate
				tokio::task::spawn_blocking(move || (coord, cb(coord)))
			})
			.buffer_unordered(num_cpus::get()) // concurrency
			.filter_map(|result| async {
				match result {
					Ok((coord, Some(item))) => Some((coord, item)),
					_ => None,
				}
			});
		TileStream { inner: s.boxed() }
	}

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
		TileStream {
			inner: Box::pin(streams.buffer_unordered(num_cpus::get()).map(|s| s.inner).flatten()),
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

	pub async fn for_each_async_parallel<F, Fut>(self, callback: F)
	where
		F: FnMut((TileCoord, T)) -> Fut,
		Fut: Future<Output = ()>,
	{
		self.inner.for_each_concurrent(num_cpus::get(), callback).await;
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
	/// Spawns tokio tasks with concurrency of `num_cpus::get()`. Each item `(coord, value)` is mapped
	/// to `(coord, callback(value))`.
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
	/// let mapped = stream.map_item_parallel(|value| {
	///     // Example transformation on the tile value
	///     Ok(Blob::from(format!("mapped {}", value.as_str())))
	/// });
	///
	/// let items = mapped.to_vec().await;
	/// // items contain the transformed data.
	/// # }
	/// ```
	pub fn map_item_parallel<F, O>(self, callback: F) -> TileStream<'a, O>
	where
		F: Fn(T) -> Result<O> + Send + Sync + 'static,
		T: 'static,
		O: Send + Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(item)))
			})
			.buffer_unordered(num_cpus::get())
			.map(|e| {
				let (coord, item) = e.unwrap();
				(
					coord,
					unwrap_result(item, || format!("Failed to process tile at {coord:?}")),
				)
			});
		TileStream { inner: s.boxed() }
	}

	pub fn flat_map_parallel<F, O>(self, callback: F) -> TileStream<'a, O>
	where
		F: Fn(TileCoord, T) -> Result<TileStream<'a, O>> + Send + Sync + 'static,
		T: 'static,
		O: 'static,
	{
		let arc_cb = Arc::new(callback);
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || {
					let s = unwrap_result(cb(coord, item), || format!("Failed to process tile at {coord:?}"));
					unsafe { std::mem::transmute::<_, TileStream<O>>(s) }
				})
			})
			.buffer_unordered(num_cpus::get())
			.flat_map_unordered(None, |e| e.unwrap().inner);
		TileStream { inner: s.boxed() }
	}

	/// Filters and transforms the **value of type `T`** for each tile in parallel, discarding items where `callback` returns `None`.
	///
	/// Spawns tokio tasks with concurrency of `num_cpus::get()`. Each item `(coord, value)` is mapped
	/// to `(coord, callback(value))`. If `callback` returns `None`, the item is dropped.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileCoord, Blob, TileStream};
	/// # async fn test() {
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
	/// let items = filtered.to_vec().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_item_parallel<F, O>(self, callback: F) -> TileStream<'a, O>
	where
		F: Fn(T) -> Result<Option<O>> + Send + Sync + 'static,
		T: 'static,
		O: Send + Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let s = self
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&arc_cb);
				tokio::task::spawn_blocking(move || (coord, cb(item)))
			})
			.buffer_unordered(num_cpus::get())
			.filter_map(|res| async move {
				let (coord, maybe_item) = res.unwrap();
				let maybe_item = unwrap_result(maybe_item, || format!("Failed to process tile at {coord:?}"));
				maybe_item.map(|item| (coord, item))
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

	/// Runs a callback for every item, e.g. for progress tracking.
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

/// Unwraps a `Result`, printing a detailed error report and terminating the program on failure.
///
/// * Every layer of context is written on its own line.
/// * If a layer exposes a `source` error, it is written on a separate indented line.
/// * After reporting, the process exits with status 1.
fn unwrap_result<T>(result: anyhow::Result<T>, context: impl FnOnce() -> String) -> T {
	match result {
		Ok(value) => value,
		Err(mut err) => {
			eprintln!("ERROR:");
			err = err.context(context());
			for (idx, cause) in err.chain().enumerate() {
				eprintln!("  {idx}: {cause}");
			}

			// Make sure the message is flushed before aborting.
			let _ = std::io::stderr().flush();
			std::process::exit(1);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use tokio::sync::Mutex;

	#[tokio::test]
	async fn should_flat_map_parallel_and_flatten_results() -> Result<()> {
		// Base stream with two coords
		let base = TileStream::from_vec(vec![
			(TileCoord::new(1, 0, 0)?, 10u32),
			(TileCoord::new(1, 1, 0)?, 20u32),
		]);

		// Each item expands to a sub-stream with two entries
		let flat = base.flat_map_parallel(|coord, val| {
			let out = vec![
				(coord, format!("a:{val}")),
				(TileCoord::new(coord.level, coord.x, coord.y + 1)?, format!("b:{val}")),
			];
			Ok(TileStream::from_vec(out))
		});

		let mut items = flat.to_vec().await;
		// Sort for deterministic assertions
		items.sort_by_key(|(c, b)| (c.x, c.y, b.as_str().to_string()));

		assert_eq!(
			items,
			[
				(TileCoord::new(1, 0, 0)?, "a:10".into()),
				(TileCoord::new(1, 0, 1)?, "b:10".into()),
				(TileCoord::new(1, 1, 0)?, "a:20".into()),
				(TileCoord::new(1, 1, 1)?, "b:20".into()),
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn should_collect_all_items_from_vec() {
		let tile_data = vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("tile1")),
		];

		let tile_stream = TileStream::from_vec(tile_data.clone());
		let collected = tile_stream.to_vec().await;

		assert_eq!(collected, tile_data);
	}

	#[tokio::test]
	async fn should_iterate_sync_over_items() {
		let tile_data = vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("tile1")),
			(TileCoord::new(2, 2, 2).unwrap(), Blob::from("tile2")),
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
			["{x:0,y:0,z:0}, tile0", "{x:1,y:1,z:1}, tile1", "{x:2,y:2,z:2}, tile2"]
		);
	}

	#[tokio::test]
	async fn should_map_coord_properly() {
		let original = TileStream::from_vec(vec![(TileCoord::new(3, 1, 2).unwrap(), Blob::from("data"))]);

		let mapped = original.map_coord(|coord| TileCoord::new(coord.level + 1, coord.x * 2, coord.y * 2).unwrap());

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
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("tile1")),
			(TileCoord::new(2, 2, 2).unwrap(), Blob::from("tile2")),
		];

		let tile_stream = TileStream::from_vec(tile_data);
		let count = tile_stream.drain_and_count().await;
		assert_eq!(count, 3, "Should drain exactly 3 items");
	}

	#[tokio::test]
	async fn should_run_for_each_buffered_in_chunks() {
		let tile_data = vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("tile1")),
			(TileCoord::new(2, 2, 2).unwrap(), Blob::from("tile2")),
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
		let tile_data = vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("zero")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("one")),
		];

		// Apply parallel mapping
		let transformed = TileStream::from_vec(tile_data.clone())
			.map_item_parallel(|blob| Ok(Blob::from(format!("mapped-{}", blob.as_str()))));

		// Collect results
		let mut items = transformed.to_vec().await;
		assert_eq!(items.len(), 2, "Expected two items after mapping");

		// Sort by coordinate level to allow for unordered execution
		items.sort_by_key(|(coord, _)| coord.level);

		// Verify that coordinates are preserved and blobs correctly mapped
		assert_eq!(items[0].0, TileCoord::new(0, 0, 0).unwrap());
		assert_eq!(items[0].1.as_str(), "mapped-zero");
		assert_eq!(items[1].0, TileCoord::new(1, 1, 1).unwrap());
		assert_eq!(items[1].1.as_str(), "mapped-one");
	}

	#[tokio::test]
	async fn should_parallel_filter_map_blob_correctly() {
		let tile_data = vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("keep0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("discard1")),
			(TileCoord::new(2, 2, 2).unwrap(), Blob::from("keep2")),
		];

		let filtered = TileStream::from_vec(tile_data).filter_map_item_parallel(|blob| {
			Ok(if blob.as_str().starts_with("discard") {
				None
			} else {
				Some(Blob::from(format!("kept-{}", blob.as_str())))
			})
		});

		let items = filtered.to_vec().await;
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
			Box::pin(async { TileStream::from_vec(vec![(TileCoord::new(0, 0, 0).unwrap(), Blob::from("sub0-0"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
			Box::pin(async { TileStream::from_vec(vec![(TileCoord::new(1, 1, 1).unwrap(), Blob::from("sub1-1"))]) })
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
		let tile_data = vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("async0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("async1")),
		];

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
		let stream = TileStream::from_vec(vec![
			(TileCoord::new(0, 0, 0).unwrap(), Blob::from("z0")),
			(TileCoord::new(1, 1, 1).unwrap(), Blob::from("z1")),
		]);

		let filtered = stream.filter_coord(|coord| async move { coord.level == 0 });
		let items = filtered.to_vec().await;

		assert_eq!(items.len(), 1);
		assert_eq!(items[0].0.level, 0);
		assert_eq!(items[0].1.as_str(), "z0");
	}

	#[tokio::test]
	async fn should_create_from_iter_coord_parallel() {
		let coords = vec![TileCoord::new(0, 0, 0).unwrap(), TileCoord::new(1, 1, 1).unwrap()];

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
		let coords = vec![TileCoord::new(0, 0, 0).unwrap(), TileCoord::new(1, 1, 1).unwrap()];

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
		let stream = TileStream::from_vec(
			(1..=6)
				.map(|i| (TileCoord::new(12, i, 0).unwrap(), i))
				.collect::<Vec<_>>(),
		);
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

		let results: Vec<_> = stream.to_vec().await;
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
				.map(|(i, v)| (TileCoord::new(12, i as u32, 0).unwrap(), v))
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

		let results: Vec<_> = stream.to_vec().await;
		assert_eq!(results.len(), 3);
		assert_eq!(counter.load(Ordering::SeqCst), 6);
		assert!(max_parallel.load(Ordering::SeqCst) > 1);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn test_for_each_async_parallel_parallelism() {
		let stream = TileStream::from_vec(
			(1..=6)
				.map(|i| (TileCoord::new(12, i, 0).unwrap(), i))
				.collect::<Vec<_>>(),
		);
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
			Box::pin(async { TileStream::from_vec(vec![(TileCoord::new(0, 0, 0).unwrap(), Blob::from("a"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
			Box::pin(async { TileStream::from_vec(vec![(TileCoord::new(1, 1, 1).unwrap(), Blob::from("b"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
		];
		let merged = TileStream::<Blob>::from_streams(stream::iter(substreams));
		let items = merged.to_vec().await;
		assert_eq!(items.len(), 2);
	}
}
