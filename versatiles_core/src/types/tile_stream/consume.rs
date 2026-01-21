use super::{ConcurrencyLimits, Future, HashMap, Stream, StreamExt, TileCoord, TileStream};

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
}
