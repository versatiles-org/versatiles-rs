//! Utility methods for TileStream.
//!
//! This module provides utility methods for coordinate transformation, filtering,
//! and buffered processing:
//!
//! | Method | Description |
//! |--------|-------------|
//! | `map_coord` | Transform tile coordinates without modifying data |
//! | `filter_coord` | Filter tiles by coordinate using async predicate |
//! | `for_each_buffered` | Process tiles in buffered chunks |
//! | `drain_and_count` | Consume stream and return count |

use super::{Future, StreamExt, TileCoord, TileStream, ready};

impl<'a, T> TileStream<'a, T>
where
	T: Send + 'a,
{
	// -------------------------------------------------------------------------
	// Coordinate Transformations
	// -------------------------------------------------------------------------

	/// Applies a synchronous coordinate transformation to each `(TileCoord, T)` item.
	///
	/// Maintains the same value of type `T`, but transforms `coord` via `callback`.
	/// This is useful for translating tiles between coordinate systems or adjusting
	/// zoom levels.
	///
	/// # Arguments
	/// * `callback` - Function that receives a `TileCoord` and returns a new `TileCoord`.
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
	/// This is useful for filtering tiles based on coordinate properties without
	/// needing to examine the tile data itself.
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

	// -------------------------------------------------------------------------
	// Buffered Processing
	// -------------------------------------------------------------------------

	/// Buffers items in chunks of size `buffer_size`, then calls `callback` with each full or final chunk.
	///
	/// Consumes the stream. Items are emitted in `(TileCoord, T)` form. This is useful
	/// for batch processing operations that are more efficient when working with
	/// multiple tiles at once.
	///
	/// # Arguments
	/// * `buffer_size` - The maximum number of items to buffer before calling callback.
	/// * `callback` - Function that receives a vector of `(TileCoord, T)` items.
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
	// Stream Consumption
	// -------------------------------------------------------------------------

	/// Drains this stream of all items, returning the total count of processed items.
	///
	/// Consumes the stream completely, useful when you only care about the count
	/// of items and not the items themselves.
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Blob;

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	// -------------------------------------------------------------------------
	// map_coord
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_map_coord() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("data0")),
			(tc(1, 1, 1), Blob::from("data1")),
		]);

		let mapped = stream.map_coord(|coord| tc(coord.level + 1, coord.x, coord.y));

		let items = mapped.to_vec().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].0.level, 1);
		assert_eq!(items[1].0.level, 2);
		// Data should be unchanged
		assert_eq!(items[0].1.as_str(), "data0");
		assert_eq!(items[1].1.as_str(), "data1");
	}

	#[tokio::test]
	async fn test_map_coord_preserves_order() {
		let stream = TileStream::from_vec((0u32..10).map(|i| (tc(10, i, 0), i)).collect());

		let mapped = stream.map_coord(|coord| tc(coord.level, coord.x * 2, coord.y));

		let items = mapped.to_vec().await;
		for (i, (coord, val)) in items.iter().enumerate() {
			let expected_x = u32::try_from(i * 2).unwrap();
			assert_eq!(coord.x, expected_x);
			assert_eq!(*val, u32::try_from(i).unwrap());
		}
	}

	// -------------------------------------------------------------------------
	// filter_coord
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_filter_coord() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("z0")),
			(tc(1, 1, 1), Blob::from("z1")),
			(tc(2, 2, 2), Blob::from("z2")),
		]);

		let filtered = stream.filter_coord(|coord| async move { coord.level <= 1 });

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 2);
		assert_eq!(items[0].0.level, 0);
		assert_eq!(items[1].0.level, 1);
	}

	#[tokio::test]
	async fn test_filter_coord_all_pass() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let filtered = stream.filter_coord(|_coord| async move { true });

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 2);
	}

	#[tokio::test]
	async fn test_filter_coord_none_pass() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a")), (tc(1, 1, 1), Blob::from("b"))]);

		let filtered = stream.filter_coord(|_coord| async move { false });

		let items = filtered.to_vec().await;
		assert_eq!(items.len(), 0);
	}

	// -------------------------------------------------------------------------
	// for_each_buffered
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_for_each_buffered() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("a")),
			(tc(1, 1, 1), Blob::from("b")),
			(tc(2, 2, 2), Blob::from("c")),
		]);

		let mut chunk_sizes = Vec::new();
		stream
			.for_each_buffered(2, |chunk| {
				chunk_sizes.push(chunk.len());
			})
			.await;

		assert_eq!(chunk_sizes, vec![2, 1]);
	}

	#[tokio::test]
	async fn test_for_each_buffered_exact_multiple() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("a")),
			(tc(1, 1, 1), Blob::from("b")),
			(tc(2, 2, 2), Blob::from("c")),
			(tc(3, 3, 3), Blob::from("d")),
		]);

		let mut chunk_sizes = Vec::new();
		stream
			.for_each_buffered(2, |chunk| {
				chunk_sizes.push(chunk.len());
			})
			.await;

		assert_eq!(chunk_sizes, vec![2, 2]);
	}

	#[tokio::test]
	async fn test_for_each_buffered_empty_stream() {
		let stream: TileStream<Blob> = TileStream::empty();

		let mut called = false;
		stream
			.for_each_buffered(2, |_chunk| {
				called = true;
			})
			.await;

		assert!(!called);
	}

	// -------------------------------------------------------------------------
	// drain_and_count
	// -------------------------------------------------------------------------

	#[tokio::test]
	async fn test_drain_and_count() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), Blob::from("a")),
			(tc(1, 1, 1), Blob::from("b")),
			(tc(2, 2, 2), Blob::from("c")),
		]);

		let count = stream.drain_and_count().await;
		assert_eq!(count, 3);
	}

	#[tokio::test]
	async fn test_drain_and_count_empty() {
		let stream: TileStream<Blob> = TileStream::empty();

		let count = stream.drain_and_count().await;
		assert_eq!(count, 0);
	}
}
