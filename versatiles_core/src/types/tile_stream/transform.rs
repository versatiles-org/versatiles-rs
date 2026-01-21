use super::{Future, Result, StreamExt, TileCoord, TileStream, ready};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
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
	///     .map_parallel_try(|_coord, blob| Ok(Blob::from(format!("processed-{}", blob.as_str()))))
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
