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
//!     let tile_data = reader.tile(&TileCoord::new(0, 0, 0)?).await?;
//!     assert!(tile_data.is_some());
//!     Ok(())
//! }
//! ```

use std::sync::Arc;

#[cfg(feature = "cli")]
use crate::TilesRuntime;
use crate::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{
	Blob, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream, compression::compress,
};
use versatiles_derive::context;

/// Enum representing different mock profiles for tile data.
#[derive(Debug, Clone, Copy)]
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
	tile_pyramid: TilePyramid,
}

impl MockReader {
	/// Creates a new mock tiles reader with the specified profile.
	#[context("creating mock reader with profile {:?}", profile)]
	pub fn new_mock_profile(profile: MockReaderProfile) -> Result<MockReader> {
		let mut tile_pyramid = TilePyramid::new_empty();
		tile_pyramid.insert_bbox(&TileBBox::from_min_and_max(2, 0, 1, 2, 3)?)?;
		tile_pyramid.insert_bbox(&TileBBox::from_min_and_max(3, 0, 2, 4, 6)?)?;
		tile_pyramid.insert_bbox(&TileBBox::new_full(4)?)?;
		tile_pyramid.insert_bbox(&TileBBox::new_full(5)?)?;
		tile_pyramid.insert_bbox(&TileBBox::new_full(6)?)?;

		MockReader::new_mock(
			tile_pyramid,
			match profile {
				MockReaderProfile::Json => {
					TileSourceMetadata::new(TileFormat::JSON, TileCompression::Uncompressed, Traversal::ANY, None)
				}
				MockReaderProfile::Png => {
					TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None)
				}
				MockReaderProfile::Pbf => {
					TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, Traversal::ANY, None)
				}
			},
		)
	}

	/// Returns a mutable reference to the TileJSON metadata.
	pub fn tilejson_mut(&mut self) -> &mut TileJSON {
		&mut self.tilejson
	}

	/// Creates a new mock tiles reader with the specified parameters.
	#[context("creating mock reader from parameters")]
	pub fn new_mock(tile_pyramid: TilePyramid, metadata: TileSourceMetadata) -> Result<MockReader> {
		let mut tilejson = TileJSON::default();
		tilejson.set_string("type", "dummy")?;
		metadata.set_tile_pyramid(tile_pyramid.clone());
		Ok(MockReader {
			metadata,
			tilejson,
			tile_pyramid,
		})
	}

	/// Internal helper to create a mock tile for the given coordinate.
	fn create_mock_tile(
		coord: &TileCoord,
		bbox_pyramid: &TilePyramid,
		format: &TileFormat,
		compression: &TileCompression,
	) -> Result<Option<Tile>> {
		use TileFormat::{JPG, JSON, MVT, PNG, WEBP};

		if !bbox_pyramid.includes_coord(coord) {
			return Ok(None);
		}

		let mut blob = match format {
			JSON => Blob::from(coord.to_json()),
			PNG => Blob::from(MOCK_BYTES_PNG.to_vec()),
			MVT => Blob::from(MOCK_BYTES_PBF.to_vec()),
			JPG => Blob::from(MOCK_BYTES_JPG.to_vec()),
			WEBP => Blob::from(MOCK_BYTES_WEBP.to_vec()),
			_ => panic!("tile format {format:?} is not implemented for MockReader"),
		};
		blob = compress(blob, compression)?;
		Ok(Some(Tile::from_blob(blob, *compression, *format)))
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

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		Ok(Arc::new(self.tile_pyramid.clone()))
	}

	#[context("fetching mock tile {:?} (format={:?}, compression={:?})", coord, self.metadata.tile_format(), self.metadata.tile_compression())]
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		Self::create_mock_tile(
			coord,
			&self.tile_pyramid,
			self.metadata.tile_format(),
			self.metadata.tile_compression(),
		)
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("mock::tile_stream {bbox:?}");

		let bbox = bbox.intersection_pyramid(&self.tile_pyramid);
		let format = *self.metadata.tile_format();
		let compression = *self.metadata.tile_compression();
		let tile_pyramid = self.tile_pyramid.clone();

		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			MockReader::create_mock_tile(&coord, &tile_pyramid, &format, &compression).ok()?
		}))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = bbox.intersection_pyramid(&self.tile_pyramid);

		Ok(TileStream::from_bbox_parallel(bbox, move |_coord| Some(())))
	}

	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
		print.add_key_value("type", &"mock").await;
		Ok(())
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
			reader.tilejson().stringify(),
			"{\"tilejson\":\"3.0.0\",\"type\":\"dummy\"}"
		);
		let blob = reader
			.tile(&TileCoord::new(4, 5, 6)?)
			.await?
			.unwrap()
			.into_blob(&TileCompression::Uncompressed)?
			.into_vec();
		assert_eq!(&blob[0..4], b"\x89PNG");
		Ok(())
	}

	#[tokio::test]
	async fn tile() {
		let test = |profile, blob| async move {
			let coord = TileCoord::new(6, 23, 45).unwrap();
			let reader = MockReader::new_mock_profile(profile).unwrap();
			let tile_uncompressed = reader
				.tile(&coord)
				.await
				.unwrap()
				.unwrap()
				.into_blob(&TileCompression::Uncompressed)
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

	#[tokio::test]
	async fn tile_stream_matches_individual_reads() -> Result<()> {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?;
		// Use a bbox that intersects the mock reader's pyramid (level 4 is full)
		let bbox = TileBBox::from_min_and_max(4, 0, 0, 3, 3)?;

		// Get all tiles via stream
		let stream = reader.tile_stream(bbox).await?;
		let stream_tiles: Vec<_> = stream.to_vec().await;
		assert_eq!(stream_tiles.len(), 16); // 4x4 grid

		// Verify each streamed tile matches individual read
		for (coord, mut tile) in stream_tiles {
			let stream_blob = tile.as_blob(reader.metadata().tile_compression())?;
			let single_blob = reader
				.tile(&coord)
				.await?
				.expect("tile should exist")
				.into_blob(reader.metadata().tile_compression())?;
			assert_eq!(
				stream_blob.as_slice(),
				single_blob.as_slice(),
				"blob mismatch at {coord:?}"
			);
		}

		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn probe() -> Result<()> {
		use versatiles_core::utils::PrettyPrint;

		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?;
		let runtime = crate::TilesRuntime::default();

		let mut printer = PrettyPrint::new();
		reader
			.probe_container(&mut printer.category("container").await, &runtime)
			.await?;
		assert_eq!(printer.stringify().await, "container:\n  type: \"mock\"\n");

		Ok(())
	}
}
