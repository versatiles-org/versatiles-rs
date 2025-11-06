//! Defines the [`OperationTrait`], the core interface for all pipeline operations.
//!
//! Each operation in the VersaTiles pipeline implements this trait, exposing a
//! consistent interface for retrieving tiles, metadata, and configuration parameters.
//! Implementations may represent data sources (read operations) or transformations
//! (processing operations) that can be chained together in a pipeline.

use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_container::Tile;
use versatiles_core::{TileBBox, TileJSON, TileStream, TilesReaderParameters, Traversal};

/// Core abstraction for all tile-producing operations in a VersaTiles pipeline.
///
/// Each operation provides access to:
/// - [`TilesReaderParameters`] — describing format, compression, and zoom constraints.
/// - [`TileJSON`] metadata for the resulting tile set.
/// - [`Traversal`] strategy that defines tile access order.
/// - An asynchronous stream of tiles via [`get_stream`].
///
/// The trait is object-safe and designed for dynamic composition of heterogeneous
/// read/transform stages.
#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	/// Returns the configuration parameters of this operation, describing tile format,
	/// compression, and supported zoom/bbox range.
	fn parameters(&self) -> &TilesReaderParameters;

	/// Returns the [`TileJSON`] metadata associated with the operation's output.
	fn tilejson(&self) -> &TileJSON;

	/// Returns the traversal strategy used for reading tiles (default: [`Traversal::ANY`]).
	///
	/// Override in implementations that enforce a specific traversal order.
	fn traversal(&self) -> &Traversal {
		&Traversal::ANY
	}

	/// Returns an asynchronous stream of tiles covering the specified bounding box.
	///
	/// Implementations should emit tiles matching the requested bbox, possibly applying
	/// transformations, filtering, or aggregation. Errors indicate I/O or processing failures.
	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>>;
}
