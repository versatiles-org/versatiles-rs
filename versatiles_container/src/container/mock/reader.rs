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
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::result::Result;
//!
//! #[tokio::test]
//! async fn test_mock_reader() -> Result<()> {
//!     let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG)?;
//!     let tile_data = reader.get_tile(&TileCoord::new(0, 0, 0)?).await?;
//!     assert!(tile_data.is_some());
//!     Ok(())
//! }
//! ```

use std::sync::Arc;

use crate::{SourceType, Tile, TileSourceTrait};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{utils::compress, *};
use versatiles_derive::context;

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
	#[context("creating mock reader with profile {:?}", profile)]
	pub fn new_mock_profile(profile: MockTilesReaderProfile) -> Result<MockTilesReader> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.set_level_bbox(TileBBox::from_min_and_max(2, 0, 1, 2, 3)?);
		bbox_pyramid.set_level_bbox(TileBBox::from_min_and_max(3, 0, 2, 4, 6)?);
		bbox_pyramid.set_level_bbox(TileBBox::new_full(4)?);
		bbox_pyramid.set_level_bbox(TileBBox::new_full(5)?);
		bbox_pyramid.set_level_bbox(TileBBox::new_full(6)?);

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
	#[context("creating mock reader from parameters")]
	pub fn new_mock(parameters: TilesReaderParameters) -> Result<MockTilesReader> {
		let mut tilejson = TileJSON::default();
		tilejson.set_string("type", "dummy")?;
		Ok(MockTilesReader { parameters, tilejson })
	}
}

#[async_trait]
impl TileSourceTrait for MockTilesReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("dummy", "dummy")
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	#[context("fetching mock tile {:?} (format={:?}, compression={:?})", coord, self.parameters.tile_format, self.parameters.tile_compression)]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		use TileFormat::*;

		if !self.parameters.bbox_pyramid.contains_coord(coord) {
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
		blob = compress(blob, self.parameters.tile_compression)?;
		Ok(Some(Tile::from_blob(blob, self.parameters.tile_compression, format)))
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		self.stream_individual_tiles(bbox).await
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

	#[tokio::test]
	async fn reader() -> Result<()> {
		let reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		assert_eq!(reader.source_type().to_string(), "container 'dummy' ('dummy')");

		assert_eq!(
			reader.tilejson().as_string(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		let blob = reader
			.get_tile(&TileCoord::new(4, 5, 6)?)
			.await?
			.unwrap()
			.into_blob(TileCompression::Uncompressed)?
			.into_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
		Ok(())
	}

	#[tokio::test]
	async fn get_tile() {
		let test = |profile, blob| async move {
			let coord = TileCoord::new(6, 23, 45).unwrap();
			let reader = MockTilesReader::new_mock_profile(profile).unwrap();
			let tile_uncompressed = reader
				.get_tile(&coord)
				.await
				.unwrap()
				.unwrap()
				.into_blob(TileCompression::Uncompressed)
				.unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(MockTilesReaderProfile::Png, Blob::from(MOCK_BYTES_PNG.to_vec())).await;
		test(MockTilesReaderProfile::Pbf, Blob::from(MOCK_BYTES_PBF.to_vec())).await;
		test(MockTilesReaderProfile::Json, Blob::from("{\"z\":6,\"x\":23,\"y\":45}")).await;
	}

	#[tokio::test]
	async fn convert_from() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::Png)?;
		MockTilesWriter::write(&mut reader).await.unwrap();
		Ok(())
	}
}
