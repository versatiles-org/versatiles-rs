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

#[cfg(feature = "cli")]
use crate::TilesRuntime;
use crate::{SourceType, Tile, TileSourceMetadata};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
#[cfg(feature = "cli")]
use versatiles_core::utils::PrettyPrint;
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileStream};

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
	/// - `tile_pyramid`: Spatial extent at each zoom level
	/// - `tile_compression`: Current output compression
	/// - `tile_format`: Tile format (PNG, JPG, MVT, etc.)
	/// - `traversal`: Preferred tile traversal order
	fn metadata(&self) -> &TileSourceMetadata;

	/// Returns the `TileJSON` metadata for this tileset.
	fn tilejson(&self) -> &TileJSON;

	/// Gets the actual tile pyramid for this source.
	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>>;

	/// Fetches a single tile at the given coordinate.
	///
	/// Returns:
	/// - `Ok(Some(tile))` if the tile exists
	/// - `Ok(None)` for gaps or empty tiles
	/// - `Err(_)` on read/processing errors
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let bbox = coord.to_tile_bbox();
		let mut stream = self.tile_stream(bbox).await?;
		Ok(stream.next().await.map(|(_, t)| t))
	}

	/// Asynchronously streams all tiles within the given bounding box.
	///
	/// Returns a [`TileStream`] of `(TileCoord, Tile)` pairs. The stream handles
	/// backpressure and supports concurrent pulls.
	///
	/// Default implementation wraps individual `tile` calls with internal synchronization.
	/// Sources that can optimize bulk reads should override this.
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>>;

	/// Streams which tile coordinates exist within the given bounding box.
	///
	/// Returns a [`TileStream`] of `(TileCoord, ())` pairs — one entry per tile
	/// that actually exists. The default reads full tiles via [`tile_stream`](Self::tile_stream)
	/// and discards the data. Container readers with index structures should override
	/// this for a cheaper enumeration.
	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		Ok(self
			.tile_size_stream(bbox)
			.await?
			.filter_map(move |_coord, _tile| Some(())))
	}

	/// Streams the stored byte sizes of all tiles within the given bounding box.
	///
	/// Returns a [`TileStream`] of `(TileCoord, u32)` pairs, where the `u32`
	/// is the size of the tile blob as stored in the container.
	///
	/// The default implementation reads tiles via [`tile_stream`](Self::tile_stream)
	/// and maps each tile to its blob length. Container readers with index-based size
	/// information should override this for better performance.
	async fn tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		let compression = *self.metadata().tile_compression();
		Ok(self.tile_stream(bbox).await?.filter_map(move |_coord, tile| {
			let blob = tile.into_blob(&compression).ok()?;
			u32::try_from(blob.len()).ok()
		}))
	}

	/// Writes source-specific container metadata, or a placeholder if not implemented.
	///
	/// This is the format-specific hook invoked during the CLI probe orchestration
	/// (see `versatiles::tools::probe`). Container readers override this to report
	/// details that only make sense for their particular format.
	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this source")
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
	use super::*;
	#[cfg(feature = "cli")]
	use crate::TilesRuntime;
	use crate::Traversal;
	#[cfg(feature = "cli")]
	use versatiles_core::utils::PrettyPrint;
	use versatiles_core::{Blob, TileCompression, TileFormat, TilePyramid};

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
				metadata: TileSourceMetadata::new(
					TileFormat::MVT,
					TileCompression::Gzip,
					Traversal::ANY,
					Some(TilePyramid::new_full_up_to(3)),
				),
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

		async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
			log::trace!("test_source::tile_stream {bbox:?}");
			let tile_compression = *self.metadata.tile_compression();
			let tile_format = *self.metadata.tile_format();
			Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_| {
				Some(Tile::from_blob(
					Blob::from("test tile data"),
					tile_compression,
					tile_format,
				))
			}))
		}

		async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
			Ok(self
				.metadata
				.tile_pyramid()
				.unwrap_or_else(|| Arc::new(TilePyramid::new_full_up_to(3))))
		}
	}

	#[tokio::test]
	async fn test_metadata() {
		let reader = TestReader::new_dummy();
		let metadata = reader.metadata();
		assert_eq!(metadata.tile_compression(), &TileCompression::Gzip);
		assert_eq!(metadata.tile_format(), &TileFormat::MVT);
		assert_eq!(metadata.traversal(), &Traversal::ANY);
	}

	#[tokio::test]
	async fn test_get_meta() -> Result<()> {
		let reader = TestReader::new_dummy();
		assert_eq!(
			reader.tilejson().stringify(),
			"{\"metadata\":\"test\",\"tilejson\":\"3.0.0\"}"
		);
		Ok(())
	}

	#[tokio::test]
	async fn test_tile_stream() -> Result<()> {
		let reader = TestReader::new_dummy();
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let stream = reader.tile_stream(bbox).await?;

		assert_eq!(stream.drain_and_count().await, 4); // Assuming 4 tiles in a 2x2 bbox
		Ok(())
	}

	#[tokio::test]
	async fn test_tile_coord_stream() -> Result<()> {
		let reader = TestReader::new_dummy();
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let coord_count = reader.tile_coord_stream(bbox).await?.drain_and_count().await;
		let tile_count = reader
			.tile_stream(TileBBox::from_min_and_max(1, 0, 0, 1, 1)?)
			.await?
			.drain_and_count()
			.await;
		assert_eq!(coord_count, tile_count);
		assert_eq!(coord_count, 4);
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_container() -> Result<()> {
		let reader = TestReader::new_dummy();
		let mut print = PrettyPrint::new();
		let runtime = TilesRuntime::default();
		reader.probe_container(&mut print, &runtime).await?;
		Ok(())
	}
}
