//! Unified interface for tile sources (readers and processors).
//!
//! This module defines [`TileSource`], the common interface for anything that produces tiles,
//! whether reading from physical containers or processing/transforming tiles from upstream sources.
//!
//! ## Design Philosophy
//!
//! Both container readers (e.g., `MBTiles`, `VersaTiles`) and tile processors (e.g., filters, format converters)
//! share the same fundamental operations:
//! - Provide metadata (`TileJSON`, metadata)
//! - Stream tiles from a bounding box
//! - Support runtime composition
//!
//! By unifying these under a single trait, we enable:
//! - Seamless composition of readers and processors
//! - Type-safe pipeline construction
//! - Clear separation between data sources and transformations

use crate::{SourceType, Tile, TileSourceMetadata};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
#[cfg(feature = "cli")]
use versatiles_core::{ProbeDepth, utils::PrettyPrint};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TileStream};

/// Shared ownership of a tile source for concurrent access.
///
/// This type alias simplifies passing tile sources across async boundaries
/// and between threads.
///
/// Note: The `Box` wrapper is required because writers use `Arc::try_unwrap`
/// to get mutable access, which requires a sized inner type.
pub type SharedTileSource = Arc<Box<dyn TileSource>>;

/// Unified object-safe interface for reading or processing tiles.
///
/// Implementors include:
/// * **Container readers**: Read tiles from physical storage (files, databases, archives)
/// * **Tile processors**: Transform tiles from upstream sources (filtering, format conversion, etc.)
/// * **Composite sources**: Combine multiple sources (stacking, merging)
///
/// The trait is object-safe to support dynamic dispatch via `Box<dyn TileSource>`,
/// enabling runtime composition of heterogeneous sources and processors.
#[async_trait]
pub trait TileSource: Debug + Send + Sync + Unpin {
	/// Returns the source type (container format, processor name, or composite).
	///
	/// This helps distinguish between:
	/// - Physical containers: `SourceType::Container("mbtiles")`
	/// - Tile processors: `SourceType::Processor("filter")`
	/// - Composite sources: `SourceType::Composite`
	fn source_type(&self) -> Arc<SourceType>;

	/// Returns runtime metadata describing the tiles this source will produce.
	///
	/// Includes:
	/// - `bbox_pyramid`: Spatial extent at each zoom level
	/// - `tile_compression`: Current output compression
	/// - `tile_format`: Tile format (PNG, JPG, MVT, etc.)
	/// - `traversal`: Preferred tile traversal order
	fn metadata(&self) -> &TileSourceMetadata;

	/// Returns the `TileJSON` metadata for this tileset.
	fn tilejson(&self) -> &TileJSON;

	/// Fetches a single tile at the given coordinate.
	///
	/// Returns:
	/// - `Ok(Some(tile))` if the tile exists
	/// - `Ok(None)` for gaps or empty tiles
	/// - `Err(_)` on read/processing errors
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let bbox = coord.to_tile_bbox();
		let mut stream = self.get_tile_stream(bbox).await?;
		Ok(stream.next().await.map(|(_, t)| t))
	}

	/// Asynchronously streams all tiles within the given bounding box.
	///
	/// Returns a [`TileStream`] of `(TileCoord, Tile)` pairs. The stream handles
	/// backpressure and supports concurrent pulls.
	///
	/// Default implementation wraps individual `get_tile` calls with internal synchronization.
	/// Sources that can optimize bulk reads should override this.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>>;

	async fn stream_individual_tiles(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		Ok(TileStream::from_coord_vec_async(
			bbox.into_iter_coords().collect(),
			async move |c| self.get_tile(&c).await.ok().flatten().map(|t| (c, t)),
		))
	}

	/// Performs a hierarchical CLI probe at the specified depth.
	///
	/// Probes metadata, container specifics, tiles, and tile contents
	/// based on the requested depth level.
	#[cfg(feature = "cli")]
	async fn probe(&self, level: ProbeDepth) -> Result<()> {
		use ProbeDepth::{Container, TileContents, Tiles};

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("source_type", &self.source_type().to_string()).await;

		cat.add_key_json("meta", &self.tilejson().as_json_value()).await;

		self.probe_metadata(&mut print.get_category("parameters").await).await?;

		if matches!(level, Container | Tiles | TileContents) {
			log::debug!("probing source {:?} at depth {:?}", self.source_type(), level);
			self.probe_container(&print.get_category("container").await).await?;
		}

		if matches!(level, Tiles | TileContents) {
			log::debug!(
				"probing tiles {:?} at depth {:?}",
				self.tilejson().as_json_value(),
				level
			);
			self.probe_tiles(&print.get_category("tiles").await).await?;
		}

		if matches!(level, TileContents) {
			log::debug!(
				"probing tile contents {:?} at depth {:?}",
				self.tilejson().as_json_value(),
				level
			);
			self
				.probe_tile_contents(&print.get_category("tile contents").await)
				.await?;
		}

		Ok(())
	}

	/// Writes source metadata (bbox pyramid, formats, compression) to the CLI reporter.
	#[cfg(feature = "cli")]
	async fn probe_metadata(&self, print: &mut PrettyPrint) -> Result<()> {
		let metadata = self.metadata();
		let p = print.get_list("bbox_pyramid").await;
		for level in metadata.bbox_pyramid.iter_levels() {
			p.add_value(level).await;
		}
		print
			.add_key_value("bbox", &format!("{:?}", metadata.bbox_pyramid.get_geo_bbox()))
			.await;
		print
			.add_key_value("tile compression", &metadata.tile_compression)
			.await;
		print.add_key_value("tile format", &metadata.tile_format).await;
		Ok(())
	}

	/// Writes source-specific metadata or a placeholder if not implemented.
	///
	/// Container readers may override to provide format-specific details.
	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Writes tile-level probing output or a placeholder if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tiles(&self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Writes sample tile content diagnostics or a placeholder if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tile_contents(&self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Converts `self` into a boxed trait object for dynamic dispatch.
	fn boxed(self) -> Box<dyn TileSource>
	where
		Self: Sized + 'static,
	{
		Box::new(self)
	}

	/// Converts `self` into a shared reference for concurrent access.
	///
	/// This is the preferred way to create a `SharedTileSource` from a concrete
	/// tile source implementation.
	fn into_shared(self) -> SharedTileSource
	where
		Self: Sized + 'static,
	{
		Arc::new(Box::new(self))
	}
}

/// Tests cover trait defaults, parameter plumbing, streaming behavior, and the CLI probe stubs.
#[cfg(test)]
mod tests {
	#[cfg(feature = "cli")]
	use super::ProbeDepth;
	use super::*;
	use crate::Traversal;
	#[cfg(feature = "cli")]
	use versatiles_core::utils::PrettyPrint;
	use versatiles_core::{Blob, TileBBoxPyramid, TileCompression, TileFormat};

	#[derive(Debug)]
	struct TestReader {
		metadata: TileSourceMetadata,
		tilejson: TileJSON,
	}

	impl TestReader {
		fn new_dummy() -> TestReader {
			let mut tilejson = TileJSON::default();
			tilejson.set_string("metadata", "test").unwrap();
			TestReader {
				metadata: TileSourceMetadata {
					bbox_pyramid: TileBBoxPyramid::new_full(3),
					tile_compression: TileCompression::Gzip,
					tile_format: TileFormat::MVT,
					traversal: Traversal::ANY,
				},
				tilejson,
			}
		}
	}

	#[async_trait]
	impl TileSource for TestReader {
		fn source_type(&self) -> Arc<SourceType> {
			SourceType::new_container("dummy_format", "dummy_uri")
		}

		fn metadata(&self) -> &TileSourceMetadata {
			&self.metadata
		}

		fn tilejson(&self) -> &TileJSON {
			&self.tilejson
		}

		async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
			let tile_compression = self.metadata.tile_compression;
			let tile_format = self.metadata.tile_format;
			Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_| {
				Some(Tile::from_blob(
					Blob::from("test tile data"),
					tile_compression,
					tile_format,
				))
			}))
		}
	}

	#[tokio::test]
	async fn test_metadata() {
		let reader = TestReader::new_dummy();
		let metadata = reader.metadata();
		assert_eq!(metadata.tile_compression, TileCompression::Gzip);
		assert_eq!(metadata.tile_format, TileFormat::MVT);
		assert_eq!(metadata.bbox_pyramid.get_level_min().unwrap(), 0);
		assert_eq!(metadata.bbox_pyramid.get_level_max().unwrap(), 3);
	}

	#[tokio::test]
	async fn test_get_meta() -> Result<()> {
		let reader = TestReader::new_dummy();
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"metadata\":\"test\",\"tilejson\":\"3.0.0\"}"
		);
		Ok(())
	}

	#[tokio::test]
	async fn test_get_tile_stream() -> Result<()> {
		let reader = TestReader::new_dummy();
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let stream = reader.get_tile_stream(bbox).await?;

		assert_eq!(stream.drain_and_count().await, 4); // Assuming 4 tiles in a 2x2 bbox
		Ok(())
	}

	#[tokio::test]
	async fn test_probe_tile_contents() -> Result<()> {
		#[cfg(feature = "cli")]
		{
			use versatiles_core::utils::PrettyPrint;

			let reader = TestReader::new_dummy();
			let mut print = PrettyPrint::new();
			reader
				.probe_tile_contents(&print.get_category("tile contents").await)
				.await?;
		}
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_metadata() -> Result<()> {
		let reader = TestReader::new_dummy();
		let mut print = PrettyPrint::new();
		reader.probe_metadata(&mut print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_container() -> Result<()> {
		let reader = TestReader::new_dummy();
		let print = PrettyPrint::new();
		reader.probe_container(&print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_tiles() -> Result<()> {
		let reader = TestReader::new_dummy();
		let print = PrettyPrint::new();
		reader.probe_tiles(&print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_all_levels() -> Result<()> {
		let reader = TestReader::new_dummy();
		reader.probe(ProbeDepth::Container).await?;
		reader.probe(ProbeDepth::Tiles).await?;
		reader.probe(ProbeDepth::TileContents).await?;
		Ok(())
	}
}
