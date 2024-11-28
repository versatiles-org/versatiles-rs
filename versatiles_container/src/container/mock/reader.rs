//! Mock implementation of tile readers for testing purposes
//!
//! This module provides a mock implementation of the `TilesReader` trait, allowing for testing of tile reading functionality without relying on actual tile data or I/O operations.
//!
//! ## MockTilesReader
//! The `MockTilesReader` struct is the main component, which can be initialized with different profiles representing various tile formats and compressions.
//!
//! ## Usage
//! These mocks can be used to simulate tile reading operations in tests, allowing verification of code behavior under controlled conditions.
//!
//! ```rust
//! use versatiles::{
//!     container::{MockTilesReader, MockTilesReaderProfile},
//!     types::TilesReaderTrait
//! };
//! use std::result::Result;
//!
//! #[tokio::test]
//! async fn test_mock_reader() -> Result<()> {
//!     let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG)?;
//!     let tile_data = reader.get_tile_data(&TileCoord3::new(0, 0, 0)?).await?;
//!     assert!(tile_data.is_some());
//!     Ok(())
//! }
//! ```

use crate::{
	types::{
		Blob, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat, TilesReaderParameters,
		TilesReaderTrait,
	},
	utils::compress,
};
use anyhow::Result;
use async_trait::async_trait;

/// Enum representing different mock profiles for tile data.
#[derive(Debug)]
pub enum MockTilesReaderProfile {
	/// Mock profile for JSON format.
	Json,
	/// Mock profile for PNG format.
	Png,
	/// Mock profile for PBF format.
	Pbf,
}

pub const MOCK_BYTES_JPG: &[u8; 671] = include_bytes!("./mock_tiles/mock.jpg");
pub const MOCK_BYTES_PBF: &[u8; 54] = include_bytes!("./mock_tiles/mock.pbf");
pub const MOCK_BYTES_PNG: &[u8; 103] = include_bytes!("./mock_tiles/mock.png");
pub const MOCK_BYTES_WEBP: &[u8; 44] = include_bytes!("./mock_tiles/mock.webp");

/// Mock implementation of a `TilesReader`.
pub struct MockTilesReader {
	parameters: TilesReaderParameters,
}

impl MockTilesReader {
	/// Creates a new mock tiles reader with the specified profile.
	pub fn new_mock_profile(profile: MockTilesReaderProfile) -> Result<MockTilesReader> {
		let bbox_pyramid = TileBBoxPyramid::new_full(4);

		MockTilesReader::new_mock(match profile {
			MockTilesReaderProfile::Json => TilesReaderParameters::new(
				TileFormat::JSON,
				TileCompression::Uncompressed,
				bbox_pyramid,
			),
			MockTilesReaderProfile::Png => {
				TilesReaderParameters::new(TileFormat::PNG, TileCompression::Uncompressed, bbox_pyramid)
			}
			MockTilesReaderProfile::Pbf => {
				TilesReaderParameters::new(TileFormat::PBF, TileCompression::Gzip, bbox_pyramid)
			}
		})
	}

	/// Creates a new mock tiles reader with the specified parameters.
	pub fn new_mock(parameters: TilesReaderParameters) -> Result<MockTilesReader> {
		Ok(MockTilesReader { parameters })
	}
}

#[async_trait]
impl TilesReaderTrait for MockTilesReader {
	fn get_container_name(&self) -> &str {
		"dummy_container"
	}

	fn get_name(&self) -> &str {
		"dummy_name"
	}

	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}

	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(Blob::from("{\"type\":\"dummy\"}")))
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		use TileFormat::*;

		if !coord.is_valid() {
			return Ok(None);
		}

		let format = self.parameters.tile_format;
		let mut blob = match format {
			JSON => Blob::from(coord.as_json()),
			PNG => Blob::from(MOCK_BYTES_PNG.to_vec()),
			PBF => Blob::from(MOCK_BYTES_PBF.to_vec()),
			//AVIF => Blob::from(MOCK_BYTES_AVIF.to_vec()),
			JPG => Blob::from(MOCK_BYTES_JPG.to_vec()),
			WEBP => Blob::from(MOCK_BYTES_WEBP.to_vec()),
			_ => panic!("tile format {format:?} is not implemented for MockTileReader"),
		};
		blob = compress(blob, &self.parameters.tile_compression)?;
		Ok(Some(blob))
	}
}

impl std::fmt::Debug for MockTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MockTilesReader")
			.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{container::MockTilesWriter, utils::decompress};
	use anyhow::Result;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		assert_eq!(reader.get_container_name(), "dummy_container");
		assert_eq!(reader.get_name(), "dummy_name");

		let bbox_pyramid = TileBBoxPyramid::new_full(4);

		assert_eq!(
			reader.get_parameters(),
			&TilesReaderParameters::new(TileFormat::PNG, TileCompression::Uncompressed, bbox_pyramid)
		);
		assert_eq!(reader.get_meta()?.unwrap().as_str(), "{\"type\":\"dummy\"}");
		let blob = reader
			.get_tile_data(&TileCoord3::new(0, 0, 0)?)
			.await?
			.unwrap()
			.into_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
		Ok(())
	}

	#[tokio::test]
	async fn get_tile_data() {
		let test = |profile, blob| async move {
			let coord = TileCoord3::new(23, 45, 6).unwrap();
			let reader = MockTilesReader::new_mock_profile(profile).unwrap();
			let tile_compressed = reader.get_tile_data(&coord).await.unwrap().unwrap();
			let tile_uncompressed =
				decompress(tile_compressed, &reader.get_parameters().tile_compression).unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(
			MockTilesReaderProfile::Png,
			Blob::from(MOCK_BYTES_PNG.to_vec()),
		)
		.await;
		test(
			MockTilesReaderProfile::Pbf,
			Blob::from(MOCK_BYTES_PBF.to_vec()),
		)
		.await;
		test(MockTilesReaderProfile::Json, Blob::from("{x:23,y:45,z:6}")).await;
	}

	#[tokio::test]
	async fn convert_from() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		MockTilesWriter::write(&mut reader).await.unwrap();
		Ok(())
	}
}
