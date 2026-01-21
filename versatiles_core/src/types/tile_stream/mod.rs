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

mod constructors;
mod consume;
mod filter_map;
mod flat_map;
mod map;
mod transform;

#[cfg(test)]
mod tests;

use crate::{Blob, ConcurrencyLimits, TileBBox, TileCoord};
use anyhow::Result;
use futures::{
	Future, Stream, StreamExt,
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
