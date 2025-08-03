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
//! use versatiles_container::{MockTilesReader, MockTilesReaderProfile};
//! use versatiles_core::TilesReaderTrait;
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

use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{tilejson::TileJSON, utils::compress, *};

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
	tilejson: TileJSON,
}

impl MockTilesReader {
	/// Creates a new mock tiles reader with the specified profile.
	pub fn new_mock_profile(profile: MockTilesReaderProfile) -> Result<MockTilesReader> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.set_level_bbox(TileBBox::new(2, 0, 1, 2, 3)?);
		bbox_pyramid.set_level_bbox(TileBBox::new(3, 0, 2, 4, 6)?);

		MockTilesReader::new_mock(match profile {
			MockTilesReaderProfile::Json => {
				TilesReaderParameters::new(TileFormat::JSON, TileCompression::Uncompressed, bbox_pyramid)
			}
			MockTilesReaderProfile::Png => {
				TilesReaderParameters::new(TileFormat::PNG, TileCompression::Uncompressed, bbox_pyramid)
			}
			MockTilesReaderProfile::Pbf => {
				TilesReaderParameters::new(TileFormat::MVT, TileCompression::Gzip, bbox_pyramid)
			}
		})
	}

	/// Creates a new mock tiles reader with the specified parameters.
	pub fn new_mock(parameters: TilesReaderParameters) -> Result<MockTilesReader> {
		let mut tilejson = TileJSON::default();
		tilejson.set_string("type", "dummy")?;
		Ok(MockTilesReader { parameters, tilejson })
	}
}

#[async_trait]
impl TilesReaderTrait for MockTilesReader {
	fn container_name(&self) -> &str {
		"dummy_container"
	}

	fn source_name(&self) -> &str {
		"dummy_name"
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
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
			MVT => Blob::from(MOCK_BYTES_PBF.to_vec()),
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
			.field("parameters", &self.parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::MockTilesWriter;
	use anyhow::Result;
	use versatiles_core::utils::decompress;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		assert_eq!(reader.container_name(), "dummy_container");
		assert_eq!(reader.source_name(), "dummy_name");

		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.set_level_bbox(TileBBox::new(2, 0, 1, 2, 3)?);
		bbox_pyramid.set_level_bbox(TileBBox::new(3, 0, 2, 4, 6)?);

		assert_eq!(
			reader.parameters(),
			&TilesReaderParameters::new(TileFormat::PNG, TileCompression::Uncompressed, bbox_pyramid)
		);
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
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
			let coord = TileCoord3::new(6, 23, 45).unwrap();
			let reader = MockTilesReader::new_mock_profile(profile).unwrap();
			let tile_compressed = reader.get_tile_data(&coord).await.unwrap().unwrap();
			let tile_uncompressed = decompress(tile_compressed, &reader.parameters().tile_compression).unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(MockTilesReaderProfile::Png, Blob::from(MOCK_BYTES_PNG.to_vec())).await;
		test(MockTilesReaderProfile::Pbf, Blob::from(MOCK_BYTES_PBF.to_vec())).await;
		test(MockTilesReaderProfile::Json, Blob::from("{x:23,y:45,z:6}")).await;
	}

	#[tokio::test]
	async fn convert_from() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		MockTilesWriter::write(&mut reader).await.unwrap();
		Ok(())
	}
}
