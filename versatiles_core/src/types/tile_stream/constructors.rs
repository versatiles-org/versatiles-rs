use super::{Arc, ConcurrencyLimits, Future, Pin, Stream, StreamExt, TileBBox, TileCoord, TileStream, ready, stream};

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

	/// Creates a `TileStream` by processing all coordinates in a `TileBBox` in parallel.
	///
	/// This is a convenience wrapper around [`from_iter_coord_parallel`](Self::from_iter_coord_parallel)
	/// that accepts a [`TileBBox`] instead of an iterator. For each coordinate in the bounding box,
	/// spawns a tokio task (buffered by CPU-bound concurrency limit) to call `callback`.
	/// Returns only items where `callback(coord)` yields `Some(value)`.
	///
	/// Coordinates are processed in parallel without guaranteed order.
	/// Uses CPU-bound concurrency limit since the callback runs in `spawn_blocking`.
	///
	/// # Arguments
	/// * `bbox` - The bounding box defining the tile coordinates to process.
	/// * `callback` - A shared closure returning `Option<T>` for each coordinate.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileBBox, TileCoord, Blob, TileStream};
	/// let bbox = TileBBox::from_min_and_max(4, 0, 0, 3, 3).unwrap();
	/// let closure = |coord: TileCoord| {
	///     // Data loading logic...
	///     Some(Blob::from(format!("data for {:?}", coord)))
	/// };
	///
	/// let tile_stream = TileStream::from_bbox_parallel(bbox, closure);
	/// ```
	pub fn from_bbox_parallel<F>(bbox: TileBBox, callback: F) -> Self
	where
		F: Fn(TileCoord) -> Option<T> + Send + Sync + 'static,
		T: 'static,
	{
		Self::from_iter_coord_parallel(bbox.into_iter_coords(), callback)
	}

	/// Creates a `TileStream` by processing all coordinates in a `TileBBox` with async callbacks in parallel.
	///
	/// For each coordinate in the bounding box, calls the async `callback` concurrently up to the
	/// cpu_bound concurrency limit. Returns only items where `callback(coord)` yields `Some(value)`.
	///
	/// Coordinates are processed in parallel without guaranteed order.
	/// Uses cpu_bound concurrency limit.
	///
	/// # Arguments
	/// * `bbox` - The bounding box defining the tile coordinates to process.
	/// * `callback` - An async closure returning `Option<(TileCoord, T)>` for each coordinate.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::{TileBBox, TileCoord, Blob, TileStream};
	/// # async fn example() {
	/// let bbox = TileBBox::from_min_and_max(4, 0, 0, 3, 3).unwrap();
	/// let tile_stream = TileStream::from_bbox_async_parallel(bbox, |coord| async move {
	///     // Async data loading logic...
	///     Some((coord, Blob::from(format!("data for {:?}", coord))))
	/// });
	/// # }
	/// ```
	pub fn from_bbox_async_parallel<F, Fut>(bbox: TileBBox, callback: F) -> Self
	where
		F: FnMut(TileCoord) -> Fut + Send + 'a,
		Fut: Future<Output = Option<(TileCoord, T)>> + Send + 'a,
	{
		let limits = ConcurrencyLimits::default();
		let s = stream::iter(bbox.into_iter_coords())
			.map(callback)
			.buffer_unordered(limits.cpu_bound)
			.filter_map(ready);
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
}
