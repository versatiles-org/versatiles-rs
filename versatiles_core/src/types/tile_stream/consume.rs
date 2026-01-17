use super::{ConcurrencyLimits, Future, HashMap, Stream, StreamExt, TileCoord, TileStream, ready};

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
		self.inner.for_each_concurrent(limits.cpu_bound, callback).await;
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
}
