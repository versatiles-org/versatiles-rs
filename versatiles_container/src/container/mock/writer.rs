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

use crate::TilesWriterTrait;
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{io::DataWriterTrait, types::TilesReaderTrait};

/// Mock implementation of a `TilesWriter`.
pub struct MockTilesWriter {}

impl MockTilesWriter {
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
	pub async fn write(reader: &mut dyn TilesReaderTrait) -> Result<()> {
		let _temp = reader.get_container_name();
		let _temp = reader.get_source_name();
		let _temp = reader.get_tilejson();

		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(*bbox).await;
			while stream.next().await.is_some() {}
		}

		Ok(())
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
	async fn write_to_writer(reader: &mut dyn TilesReaderTrait, _writer: &mut dyn DataWriterTrait) -> Result<()> {
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
