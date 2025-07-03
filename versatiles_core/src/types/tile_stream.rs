//! A module defining the `TileStream` struct, which provides asynchronous handling of a stream of tiles.
//!
//! Each tile is represented by a coordinate (`TileCoord3`) and its data (`Blob`). The `TileStream`
//! offers methods for parallel processing, buffering, synchronization callbacks, and easy iteration.
//!
//! # Features
//! - **Parallel Processing**: Transform or filter tile data in parallel using tokio tasks.
//! - **Buffering**: Collect or process data in configurable batches.
//! - **Synchronous and Asynchronous Callbacks**: Choose between sync and async processing steps.

use crate::types::{Blob, TileCoord3};
use anyhow::Result;
use futures::{
	future::ready,
	stream::{self, BoxStream},
	Future, Stream, StreamExt,
};
use std::{io::Write, pin::Pin, sync::Arc};

/// A wrapper that encapsulates a stream of `(TileCoord3, Blob)` tuples.
///
/// Each item in the stream represents a tile coordinate and its associated data.
/// Methods are provided for parallel transformation, buffering, and iteration.
///
/// The `'a` lifetime parameter ensures that data from external iterators or references
/// remains valid throughout the stream’s usage.
pub struct TileStream<'a> {
	/// The internal boxed stream, emitting `(TileCoord3, Blob)` pairs.
	pub stream: BoxStream<'a, (TileCoord3, Blob)>,
}

#[allow(dead_code)]
impl<'a> TileStream<'a> {
	// -------------------------------------------------------------------------
	// Constructors
	// -------------------------------------------------------------------------

	/// Creates a `TileStream` containing no items.
	///
	/// Useful for representing an empty data source.
	pub fn new_empty() -> Self {
		Self {
			stream: stream::empty().boxed(),
		}
	}

	/// Creates a `TileStream` from an existing `Stream` of `(TileCoord3, Blob)`.
	///
	/// # Examples
	/// ```
	/// use futures::{stream, StreamExt};
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	///
	/// let tile_data = stream::iter(vec![
	///     (TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile0")),
	///     (TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile1")),
	/// ]);
	/// let my_stream = TileStream::from_stream(tile_data.boxed());
	/// ```
	pub fn from_stream(stream: Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>) -> Self {
		TileStream { stream }
	}

	/// Constructs a `TileStream` from a static vector of `(TileCoord3, Blob)` items.
	///
	/// The resulting stream will yield each item in `vec` in order.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// let tile_data = vec![
	///     (TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile0")),
	///     (TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile1")),
	/// ];
	/// let tile_stream = TileStream::from_vec(tile_data);
	/// ```
	pub fn from_vec(vec: Vec<(TileCoord3, Blob)>) -> Self {
		TileStream {
			stream: stream::iter(vec).boxed(),
		}
	}

	// -------------------------------------------------------------------------
	// Stream Creation from Iterators
	// -------------------------------------------------------------------------

	/// Creates a `TileStream` by converting an iterator of `TileCoord3` into parallel tasks
	/// that produce `(TileCoord3, Blob)` items asynchronously.
	///
	/// Spawns one tokio task per coordinate (buffered by `num_cpus::get()`), calling `callback`
	/// to produce the tile data. Returns only items where `callback(coord)` yields `Some(blob)`.
	///
	/// # Arguments
	/// * `iter` - An iterator of tile coordinates.
	/// * `callback` - A shared closure returning `Option<Blob>` for each coordinate.
	///
	/// # Examples
	/// ```
	/// # use std::sync::Arc;
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// let coords = vec![TileCoord3::new(0,0,0).unwrap(), TileCoord3::new(1,1,1).unwrap()];
	/// let closure = |coord: TileCoord3| {
	///     // Data loading logic...
	///     Some(Blob::from(format!("data for {:?}", coord)))
	/// };
	///
	/// let tile_stream = TileStream::from_coord_iter_parallel(coords.into_iter(), closure);
	/// ```
	pub fn from_coord_iter_parallel<F>(iter: impl Iterator<Item = TileCoord3> + Send + 'a, callback: F) -> Self
	where
		F: Fn(TileCoord3) -> Option<Blob> + Send + Sync + 'static,
	{
		let callback = Arc::new(callback);
		let s = stream::iter(iter)
			.map(move |coord| {
				let c = Arc::clone(&callback);
				// Spawn a task for each coordinate
				tokio::spawn(async move { (coord, c(coord)) })
			})
			.buffer_unordered(num_cpus::get()) // concurrency
			.filter_map(|result| async {
				match result {
					Ok((coord, Some(blob))) => Some((coord, blob)),
					_ => None,
				}
			});
		TileStream { stream: s.boxed() }
	}

	/// Creates a `TileStream` by filtering and mapping an async closure over a vector of tile coordinates.
	///
	/// The closure `callback` takes a coordinate and returns a `Future` that yields
	/// an `Option<(TileCoord3, Blob)>`. Only `Some` items are emitted.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # use futures::Future;
	/// # async fn example() {
	/// let coords = vec![TileCoord3::new(0,0,0).unwrap(), TileCoord3::new(1,1,1).unwrap()];
	/// let closure = |coord: TileCoord3| async move {
	///     if coord.z == 0 {
	///         Some((coord, Blob::from("data")))
	///     } else {
	///         None
	///     }
	/// };
	///
	/// let tile_stream = TileStream::from_coord_vec_async(coords, closure);
	/// let items = tile_stream.collect().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn from_coord_vec_async<F, Fut>(vec: Vec<TileCoord3>, callback: F) -> Self
	where
		F: FnMut(TileCoord3) -> Fut + Send + 'a,
		Fut: Future<Output = Option<(TileCoord3, Blob)>> + Send + 'a,
	{
		let s = stream::iter(vec).filter_map(callback);
		TileStream { stream: s.boxed() }
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
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # use futures::future;
	/// #
	/// async fn example(tile_streams: Vec<impl std::future::Future<Output=TileStream<'static>> + Send + 'static>) {
	///     let merged = TileStream::from_stream_iter(tile_streams.into_iter()).await;
	///     let all_items = merged.collect().await;
	///     // `all_items` now contains items from all child streams
	/// }
	/// ```
	pub async fn from_stream_iter<Fut>(iter: impl Iterator<Item = Fut> + Send + 'a) -> TileStream<'a>
	where
		Fut: Future<Output = TileStream<'a>> + Send + 'a,
	{
		TileStream {
			// Wait for each future -> flatten all streams
			stream: Box::pin(stream::iter(iter).then(|s| async move { s.await.stream }).flatten()),
		}
	}

	// -------------------------------------------------------------------------
	// Collecting and Iteration
	// -------------------------------------------------------------------------

	/// Collects all `(TileCoord3, Blob)` items from this stream into a vector.
	///
	/// Consumes the stream.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	/// let items = stream.collect().await;
	/// assert_eq!(items.len(), 2);
	/// # }
	/// ```
	pub async fn collect(self) -> Vec<(TileCoord3, Blob)> {
		self.stream.collect().await
	}

	/// Retrieves the next `(TileCoord3, Blob)` item from this stream, or `None` if the stream is empty.
	///
	/// The internal pointer advances by one item.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let mut stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
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
	pub async fn next(&mut self) -> Option<(TileCoord3, Blob)> {
		self.stream.next().await
	}

	/// Applies an asynchronous callback `callback` to each `(TileCoord3, Blob)` item.
	///
	/// Consumes the stream. The provided closure returns a `Future<Output=()>`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # use futures::Future;
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_async(|(coord, blob)| async move {
	///     println!("coord={:?}, blob={:?}", coord, blob);
	/// }).await;
	/// # }
	/// ```
	pub async fn for_each_async<F, Fut>(self, callback: F)
	where
		F: FnMut((TileCoord3, Blob)) -> Fut,
		Fut: Future<Output = ()>,
	{
		self.stream.for_each(callback).await;
	}

	/// Applies a synchronous callback `callback` to each `(TileCoord3, Blob)` item.
	///
	/// Consumes the stream. The provided closure returns `()`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// stream.for_each_sync(|(coord, blob)| {
	///     println!("coord={:?}, blob={:?}", coord, blob);
	/// }).await;
	/// # }
	/// ```
	pub async fn for_each_sync<F>(self, mut callback: F)
	where
		F: FnMut((TileCoord3, Blob)),
	{
		self
			.stream
			.for_each(|item| {
				callback(item);
				ready(())
			})
			.await;
	}

	/// Buffers items in chunks of size `buffer_size`, then calls `callback` with each full or final chunk.
	///
	/// Consumes the stream.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	///     (TileCoord3::new(2,2,2).unwrap(), Blob::from("data2")),
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
		F: FnMut(Vec<(TileCoord3, Blob)>),
	{
		let mut buffer = Vec::with_capacity(buffer_size);
		while let Some(item) = self.stream.next().await {
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

	/// Transforms the `Blob` portion of each tile in parallel using the provided closure `callback`.
	///
	/// Spawns tokio tasks with concurrency of `num_cpus::get()`. Each item `(coord, blob)` is mapped
	/// to `(coord, callback(blob))`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped = stream.map_blob_parallel(|blob| {
	///     // Example transformation
	///     Ok(Blob::from(format!("mapped {}", blob.as_str())))
	/// });
	///
	/// let items = mapped.collect().await;
	/// // items contain the transformed data.
	/// # }
	/// ```
	pub fn map_blob_parallel<F>(self, callback: F) -> Self
	where
		F: Fn(Blob) -> Result<Blob> + Send + Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let s = self
			.stream
			.map(move |(coord, blob)| {
				let cb = Arc::clone(&arc_cb);
				tokio::spawn(async move { (coord, cb(blob)) })
			})
			.buffer_unordered(num_cpus::get())
			.map(|e| {
				let (coord, blob) = e.unwrap();
				(
					coord,
					unwrap_result(blob, || format!("Failed to process tile at {coord:?}")),
				)
			});
		TileStream { stream: s.boxed() }
	}

	/// Filters and transforms the `Blob` portion of each tile in parallel, discarding items where `callback` returns `None`.
	///
	/// Spawns tokio tasks with concurrency of `num_cpus::get()`. Each item `(coord, blob)` is mapped
	/// to `(coord, callback(blob))`. If `callback` returns `None`, the item is dropped.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("keep")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("discard")),
	/// ]);
	///
	/// let filtered = stream.filter_map_blob_parallel(|blob| {
	///     Ok(if blob.as_str() == "discard" {
	///         None
	///     } else {
	///         Some(Blob::from(format!("was: {}", blob.as_str())))
	///     })
	/// });
	///
	/// let items = filtered.collect().await;
	/// assert_eq!(items.len(), 1);
	/// # }
	/// ```
	pub fn filter_map_blob_parallel<F>(self, callback: F) -> Self
	where
		F: Fn(Blob) -> Result<Option<Blob>> + Send + Sync + 'static,
	{
		let arc_cb = Arc::new(callback);
		let s = self
			.stream
			.map(move |(coord, blob)| {
				let cb = Arc::clone(&arc_cb);
				tokio::spawn(async move { (coord, cb(blob)) })
			})
			.buffer_unordered(num_cpus::get())
			.filter_map(|res| async move {
				let (coord, maybe_blob) = res.unwrap();
				let maybe_blob = unwrap_result(maybe_blob, || format!("Failed to process tile at {coord:?}"));
				maybe_blob.map(|blob| (coord, blob))
			});
		TileStream { stream: s.boxed() }
	}

	// -------------------------------------------------------------------------
	// Coordinate Transformations
	// -------------------------------------------------------------------------

	/// Applies a synchronous coordinate transformation to each `(TileCoord3, Blob)` item.
	///
	/// Maintains the same `Blob`, but transforms `coord` via `callback`.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let mapped_coords = stream.map_coord(|coord| {
	///     TileCoord3::new(coord.x, coord.y, coord.z + 1).unwrap()
	/// });
	///
	/// let items = mapped_coords.collect().await;
	/// // The tile data remains the same, but each coordinate has its z incremented.
	/// # }
	/// ```
	pub fn map_coord<F>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord3) -> TileCoord3 + Send + 'a,
	{
		let s = self.stream.map(move |(coord, blob)| (callback(coord), blob)).boxed();
		TileStream { stream: s }
	}

	// -------------------------------------------------------------------------
	// Utility
	// -------------------------------------------------------------------------

	/// Drains this stream of all items, returning the total count of processed items.
	///
	/// # Examples
	/// ```
	/// # use versatiles_core::types::{TileCoord3, Blob, TileStream};
	/// # async fn test() {
	/// let stream = TileStream::from_vec(vec![
	///     (TileCoord3::new(0,0,0).unwrap(), Blob::from("data0")),
	///     (TileCoord3::new(1,1,1).unwrap(), Blob::from("data1")),
	/// ]);
	///
	/// let count = stream.drain_and_count().await;
	/// assert_eq!(count, 2);
	/// # }
	/// ```
	pub async fn drain_and_count(self) -> u64 {
		let mut count = 0u64;
		self
			.stream
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
	use tokio::sync::Mutex;

	use super::*;

	#[tokio::test]
	async fn should_collect_all_items_from_vec() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile1")),
		];

		let tile_stream = TileStream::from_vec(tile_data.clone());
		let collected = tile_stream.collect().await;

		assert_eq!(collected, tile_data);
	}

	#[tokio::test]
	async fn should_iterate_sync_over_items() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile1")),
		];

		let tile_stream = TileStream::from_vec(tile_data);
		let mut count = 0u64;

		tile_stream
			.for_each_sync(|(coord, blob)| {
				println!("Synchronous processing: coord={coord:?}, blob={blob:?}");
				count += 1;
			})
			.await;

		assert_eq!(count, 2, "Expected to process exactly 2 tiles");
	}

	#[tokio::test]
	async fn should_map_coord_properly() {
		let original = TileStream::from_vec(vec![(TileCoord3::new(1, 2, 3).unwrap(), Blob::from("data"))]);

		let mapped = original.map_coord(|coord| TileCoord3::new(coord.x * 2, coord.y * 2, coord.z + 1).unwrap());

		let items = mapped.collect().await;
		assert_eq!(items.len(), 1);
		let (coord, blob) = &items[0];
		assert_eq!(coord.x, 2);
		assert_eq!(coord.y, 4);
		assert_eq!(coord.z, 4);
		assert_eq!(blob.as_str(), "data");
	}

	#[tokio::test]
	async fn should_count_items_with_drain_and_count() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile1")),
			(TileCoord3::new(2, 2, 2).unwrap(), Blob::from("tile2")),
		];

		let tile_stream = TileStream::from_vec(tile_data);
		let count = tile_stream.drain_and_count().await;
		assert_eq!(count, 3, "Should drain exactly 3 items");
	}

	#[tokio::test]
	async fn should_run_for_each_buffered_in_chunks() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile0")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile1")),
			(TileCoord3::new(2, 2, 2).unwrap(), Blob::from("tile2")),
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
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("zero")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("one")),
		];

		let transformed = TileStream::from_vec(tile_data).map_blob_parallel(|blob| {
			// For demonstration, add a prefix
			Ok(Blob::from(format!("mapped-{}", blob.as_str())))
		});

		let items = transformed.collect().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "mapped-zero");
		assert_eq!(items[1].1.as_str(), "mapped-one");
	}

	#[tokio::test]
	async fn should_parallel_filter_map_blob_correctly() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("keep0")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("discard1")),
			(TileCoord3::new(2, 2, 2).unwrap(), Blob::from("keep2")),
		];

		let filtered = TileStream::from_vec(tile_data).filter_map_blob_parallel(|blob| {
			Ok(if blob.as_str().starts_with("discard") {
				None
			} else {
				Some(Blob::from(format!("kept-{}", blob.as_str())))
			})
		});

		let items = filtered.collect().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].1.as_str(), "kept-keep0");
		assert_eq!(items[1].1.as_str(), "kept-keep2");
	}

	#[tokio::test]
	async fn should_construct_empty_stream() {
		let empty = TileStream::new_empty();
		let collected = empty.collect().await;
		assert!(collected.is_empty());
	}

	#[tokio::test]
	async fn should_construct_from_stream_iter() {
		// Create multiple sub-streams
		let substreams = vec![
			Box::pin(async { TileStream::from_vec(vec![(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("sub0-0"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
			Box::pin(async { TileStream::from_vec(vec![(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("sub1-1"))]) })
				as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
		];

		// Merge them
		let merged = TileStream::from_stream_iter(substreams.into_iter()).await;
		let items = merged.collect().await;
		assert_eq!(items.len(), 2);
	}

	#[tokio::test]
	async fn should_return_none_if_stream_is_empty() {
		let mut empty = TileStream::new_empty();
		assert!(empty.next().await.is_none());
	}

	#[tokio::test]
	async fn should_process_async_for_each() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("async0")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("async1")),
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
}
