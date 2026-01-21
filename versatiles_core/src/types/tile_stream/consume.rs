use super::{ConcurrencyLimits, Future, HashMap, Result, Stream, StreamExt, TileCoord, TileStream};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
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
}

// -------------------------------------------------------------------------
// Result Handling
// -------------------------------------------------------------------------

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
