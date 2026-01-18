//! Mock implementation of tile readers for testing purposes
//!
//! This module provides a mock implementation of the `TilesReader` trait, allowing for testing of tile reading functionality without relying on actual tile data or I/O operations.
//!
//! ## MockReader
//! The `MockReader` struct is the main component, which can be initialized with different profiles representing various tile formats and compressions.
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
//!     let mut reader = MockReader::new_mock_profile(MockReaderProfile::PNG)?;
//!     let tile_data = reader.get_tile(&TileCoord::new(0, 0, 0)?).await?;
//!     assert!(tile_data.is_some());
//!     Ok(())
//! }
//! ```

use std::sync::Arc;

use crate::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{
	Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, TileStream, compression::compress,
};
use versatiles_derive::context;

/// Enum representing different mock profiles for tile data.
#[derive(Debug)]
pub enum MockReaderProfile {
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
pub struct MockReader {
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl MockReader {
	/// Creates a new mock tiles reader with the specified profile.
	#[context("creating mock reader with profile {:?}", profile)]
	pub fn new_mock_profile(profile: MockReaderProfile) -> Result<MockReader> {
		let mut bbox_pyramid = TileBBoxPyramid::new_empty();
		bbox_pyramid.set_level_bbox(TileBBox::from_min_and_max(2, 0, 1, 2, 3)?);
		bbox_pyramid.set_level_bbox(TileBBox::from_min_and_max(3, 0, 2, 4, 6)?);
		bbox_pyramid.set_level_bbox(TileBBox::new_full(4)?);
		bbox_pyramid.set_level_bbox(TileBBox::new_full(5)?);
		bbox_pyramid.set_level_bbox(TileBBox::new_full(6)?);

		MockReader::new_mock(match profile {
			MockReaderProfile::Json => TileSourceMetadata::new(
				TileFormat::JSON,
				TileCompression::Uncompressed,
				bbox_pyramid,
				Traversal::ANY,
			),
			MockReaderProfile::Png => TileSourceMetadata::new(
				TileFormat::PNG,
				TileCompression::Uncompressed,
				bbox_pyramid,
				Traversal::ANY,
			),
			MockReaderProfile::Pbf => {
				TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, bbox_pyramid, Traversal::ANY)
			}
		})
	}

	/// Creates a new mock tiles reader with the specified parameters.
	#[context("creating mock reader from parameters")]
	pub fn new_mock(metadata: TileSourceMetadata) -> Result<MockReader> {
		let mut tilejson = TileJSON::default();
		tilejson.set_string("type", "dummy")?;
		Ok(MockReader { metadata, tilejson })
	}

	/// Internal helper to create a mock tile for the given coordinate.
	fn create_mock_tile(
		coord: &TileCoord,
		bbox_pyramid: &TileBBoxPyramid,
		format: TileFormat,
		compression: TileCompression,
	) -> Result<Option<Tile>> {
		use TileFormat::{JPG, JSON, MVT, PNG, WEBP};

		if !bbox_pyramid.contains_coord(coord) {
			return Ok(None);
		}

		let mut blob = match format {
			JSON => Blob::from(coord.as_json()),
			PNG => Blob::from(MOCK_BYTES_PNG.to_vec()),
			MVT => Blob::from(MOCK_BYTES_PBF.to_vec()),
			JPG => Blob::from(MOCK_BYTES_JPG.to_vec()),
			WEBP => Blob::from(MOCK_BYTES_WEBP.to_vec()),
			_ => panic!("tile format {format:?} is not implemented for MockReader"),
		};
		blob = compress(blob, compression)?;
		Ok(Some(Tile::from_blob(blob, compression, format)))
	}
}

#[async_trait]
impl TileSource for MockReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("dummy", "dummy")
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	#[context("fetching mock tile {:?} (format={:?}, compression={:?})", coord, self.metadata.tile_format, self.metadata.tile_compression)]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		Self::create_mock_tile(
			coord,
			&self.metadata.bbox_pyramid,
			self.metadata.tile_format,
			self.metadata.tile_compression,
		)
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let format = self.metadata.tile_format;
		let compression = self.metadata.tile_compression;
		let bbox_pyramid = self.metadata.bbox_pyramid.clone();

		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			MockReader::create_mock_tile(&coord, &bbox_pyramid, format, compression).ok()?
		}))
	}
}

impl std::fmt::Debug for MockReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MockReader")
			.field("parameters", &self.metadata())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::MockWriter;
	use anyhow::Result;

	#[tokio::test]
	async fn reader() -> Result<()> {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?;
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
			let reader = MockReader::new_mock_profile(profile).unwrap();
			let tile_uncompressed = reader
				.get_tile(&coord)
				.await
				.unwrap()
				.unwrap()
				.into_blob(TileCompression::Uncompressed)
				.unwrap();
			assert_eq!(tile_uncompressed, blob);
		};

		test(MockReaderProfile::Png, Blob::from(MOCK_BYTES_PNG.to_vec())).await;
		test(MockReaderProfile::Pbf, Blob::from(MOCK_BYTES_PBF.to_vec())).await;
		test(MockReaderProfile::Json, Blob::from("{\"z\":6,\"x\":23,\"y\":45}")).await;
	}

	#[tokio::test]
	async fn convert_from() -> Result<()> {
		let mut reader = MockReader::new_mock_profile(MockReaderProfile::Png)?;
		MockWriter::write(&mut reader).await.unwrap();
		Ok(())
	}
}
