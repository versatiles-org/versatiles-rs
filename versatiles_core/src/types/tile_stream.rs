use crate::types::{Blob, TileCoord3};
use futures::{future::ready, stream, Future, Stream, StreamExt};
use std::{pin::Pin, sync::Arc};

/// A wrapper to handle streams of tiles, where each item is a tuple containing a tile coordinate and its associated data.
pub struct TileStream<'a> {
	stream: Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>,
}

#[allow(dead_code)]
impl<'a> TileStream<'a> {
	pub fn new_empty() -> Self {
		TileStream {
			stream: futures::stream::empty().boxed(),
		}
	}

	pub async fn collect(self) -> Vec<(TileCoord3, Blob)> {
		self.stream.collect().await
	}

	pub async fn next(&mut self) -> Option<(TileCoord3, Blob)> {
		self.stream.next().await
	}

	pub async fn for_each_async<F, Fut>(self, callback: F)
	where
		F: FnMut((TileCoord3, Blob)) -> Fut,
		Fut: Future<Output = ()>,
	{
		self.stream.for_each(callback).await;
	}

	pub async fn for_each_sync<F>(self, mut callback: F)
	where
		F: FnMut((TileCoord3, Blob)),
	{
		self
			.stream
			.for_each(|e| {
				callback(e);
				ready(())
			})
			.await;
	}

	pub async fn for_each_buffered<F>(mut self, buffer_size: usize, mut callback: F)
	where
		F: FnMut(Vec<(TileCoord3, Blob)>),
	{
		let mut buffer = Vec::new();
		while let Some((coord, blob)) = self.stream.next().await {
			buffer.push((coord, blob));

			if buffer.len() >= buffer_size {
				callback(buffer);
				buffer = Vec::new();
			}
		}
		if !buffer.is_empty() {
			callback(buffer);
		}
	}

	pub fn from_stream(stream: Pin<Box<dyn Stream<Item = (TileCoord3, Blob)> + Send + 'a>>) -> Self {
		TileStream { stream }
	}

	pub fn from_vec(vec: Vec<(TileCoord3, Blob)>) -> Self {
		TileStream {
			stream: Box::pin(stream::iter(vec)),
		}
	}

	pub fn from_coord_vec_async<F, Fut>(vec: Vec<TileCoord3>, callback: F) -> Self
	where
		F: FnMut(TileCoord3) -> Fut + Send + 'a,
		Fut: Future<Output = Option<(TileCoord3, Blob)>> + Send + 'a,
	{
		TileStream {
			stream: Box::pin(stream::iter(vec).filter_map(callback)),
		}
	}

	pub fn from_coord_vec_sync<F>(vec: Vec<TileCoord3>, mut callback: F) -> Self
	where
		F: FnMut(TileCoord3) -> Option<(TileCoord3, Blob)> + Send + 'a,
	{
		TileStream {
			stream: Box::pin(stream::iter(vec).filter_map(move |coord| ready(callback(coord)))),
		}
	}

	pub fn map_blob_parallel<F>(self, callback: F) -> Self
	where
		F: Fn(Blob) -> Blob + Send + Sync + 'static,
	{
		let callback = Arc::new(callback);
		TileStream {
			stream: self
				.stream
				.map(move |(coord, blob)| {
					let callback = Arc::clone(&callback);
					tokio::spawn(async move { (coord, callback(blob)) })
				})
				.buffer_unordered(num_cpus::get())
				.map(|e| e.unwrap())
				.boxed(),
		}
	}

	pub fn filter_map_blob_parallel<F>(self, callback: F) -> Self
	where
		F: Fn(Blob) -> Option<Blob> + Send + Sync + 'static,
	{
		let callback = Arc::new(callback);
		TileStream {
			stream: self
				.stream
				.map(move |(coord, blob)| {
					let callback = Arc::clone(&callback);
					tokio::spawn(async move { (coord, callback(blob)) })
				})
				.buffer_unordered(num_cpus::get())
				.filter_map(|e| async {
					let (coord, option) = e.unwrap();
					option.map(|blob| (coord, blob))
				})
				.boxed(),
		}
	}

	pub fn map_coord<F>(self, mut callback: F) -> Self
	where
		F: FnMut(TileCoord3) -> TileCoord3 + Send + 'a,
	{
		TileStream {
			stream: self
				.stream
				.map(move |(coord, blob)| (callback(coord), blob))
				.boxed(),
		}
	}

	pub async fn drain_and_count(self) -> u64 {
		let mut count = 0;
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

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn from_vec() {
		let tile_data = vec![
			(TileCoord3::new(0, 0, 0).unwrap(), Blob::from("tile1")),
			(TileCoord3::new(1, 1, 1).unwrap(), Blob::from("tile2")),
		];

		let tiles_stream = TileStream::from_vec(tile_data);

		let mut count = 0;
		tiles_stream
			.for_each_sync(|(coord, blob)| {
				println!(
					"Processing tile at coord: {:?}, with data: {:?}",
					coord, blob
				);
				count += 1;
			})
			.await;

		assert_eq!(count, 2);
	}
}
