//! Mock implementation of tile writers for testing purposes
//!
//! This module provides a mock implementation of the `TilesWriter` trait, allowing for testing of tile writing functionality without actual file I/O operations.
//!
//! ## MockTilesWriter
//! The `MockTilesWriter` struct is the main component, which provides methods to simulate the writing of tile data.
//!
//! ## Usage
//! These mocks can be used to simulate tile writing operations in tests, allowing verification of code behavior under controlled conditions.
//!
//! ```rust
//! use versatiles_container::{MockTilesReader, MockTilesReaderProfile, MockTilesWriter, TilesWriterTrait};
//! use anyhow::Result;
//!
//! #[tokio::test]
//! async fn test_mock_writer() -> Result<()> {
//!     let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG)?;
//!     MockTilesWriter::write(&mut reader).await?;
//!     Ok(())
//! }
//! ```

use crate::{TileSourceTrait, TileSourceTraverseExt, TilesRuntime, TilesWriterTrait, Traversal};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::io::DataWriterTrait;
use versatiles_derive::context;

/// Mock implementation of a `TilesWriter`.
pub struct MockTilesWriter {}

impl MockTilesWriter {
	#[context("mock writing tiles from reader '{}'", reader.source_type())]
	/// Simulates writing tile data from the given `TilesReader`.
	///
	/// This method iterates through the tile data provided by the reader and simulates the writing process.
	///
	/// # Arguments
	///
	/// * `reader` - A mutable reference to a `TilesReader` instance.
	///
	/// # Returns
	///
	/// A `Result` indicating the success or failure of the operation.
	pub async fn write(reader: &mut dyn TileSourceTrait) -> Result<()> {
		let _temp = reader.tilejson();

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, mut stream| {
					Box::pin(async move {
						while stream.next().await.is_some() {}
						Ok(())
					})
				},
				TilesRuntime::default(),
				None,
			)
			.await
	}
}

#[async_trait]
impl TilesWriterTrait for MockTilesWriter {
	/// Writes tile data from a `TilesReader` to a `DataWriterTrait`.
	///
	/// This method is not implemented for the mock writer and simply calls `MockTilesWriter::write`.
	///
	/// # Arguments
	///
	/// * `reader` - A mutable reference to a `TilesReader` instance.
	/// * `_writer` - A mutable reference to a `DataWriterTrait` instance.
	///
	/// # Returns
	///
	/// A `Result` indicating the success or failure of the operation.
	#[context("mock writing tiles to DataWriter")]
	async fn write_to_writer(
		reader: &mut dyn TileSourceTrait,
		_writer: &mut dyn DataWriterTrait,
		_runtime: TilesRuntime,
	) -> Result<()> {
		MockTilesWriter::write(reader).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockTilesReader, MockTilesReaderProfile};

	#[tokio::test]
	async fn convert_png() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		MockTilesWriter::write(&mut reader).await?;
		Ok(())
	}

	#[tokio::test]
	async fn convert_pbf() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Pbf)?;
		MockTilesWriter::write(&mut reader).await?;
		Ok(())
	}
}
