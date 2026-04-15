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
		let compression = self.metadata().tile_compression;
		Ok(self.tile_stream(bbox).await?.filter_map(move |_coord, tile| {
			let blob = tile.into_blob(compression).ok()?;
			u32::try_from(blob.len()).ok()
		}))
	}

	/// Performs a hierarchical CLI probe at the specified depth.
	///
	/// Probes metadata, container specifics, tiles, and tile contents
	/// based on the requested depth level.
	#[cfg(feature = "cli")]
	async fn probe(&self, level: ProbeDepth, runtime: &TilesRuntime) -> Result<()> {
		use ProbeDepth::{Container, TileContents, Tiles};

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("source_type", &self.source_type().to_string()).await;

		cat.add_key_json("meta", &self.tilejson().as_json_value()).await;

		self.probe_metadata(&mut print.get_category("parameters").await).await?;

		if matches!(level, Container | Tiles | TileContents) {
			log::debug!("probing source {:?} at depth {:?}", self.source_type(), level);
			self
				.probe_container(&mut print.get_category("container").await, runtime)
				.await?;
		}

		if matches!(level, Tiles | TileContents) {
			log::debug!(
				"probing tiles {:?} at depth {:?}",
				self.tilejson().as_json_value(),
				level
			);
			self
				.probe_tiles(&mut print.get_category("tiles").await, runtime)
				.await?;
		}

		if matches!(level, TileContents) {
			log::debug!(
				"probing tile contents {:?} at depth {:?}",
				self.tilejson().as_json_value(),
				level
			);
			self
				.probe_tile_contents(&mut print.get_category("tile contents").await, runtime)
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
			p.add_value(&level).await;
		}
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
	async fn probe_container(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Scans all tiles, reporting average size and the top-10 biggest tiles.
	#[cfg(feature = "cli")]
	#[allow(clippy::too_many_lines)]
	async fn probe_tiles(&self, print: &mut PrettyPrint, runtime: &TilesRuntime) -> Result<()> {
		fn format_integer_str(value: u64) -> String {
			let s = value.to_string();
			let mut result = String::new();
			for (i, c) in s.chars().enumerate() {
				if i > 0 && (s.len() - i).is_multiple_of(3) {
					result.push('_');
				}
				result.push(c);
			}
			result
		}

		#[derive(Debug)]
		#[allow(dead_code)]
		struct Entry {
			size: u64,
			x: u32,
			y: u32,
			z: u8,
		}

		let mut biggest_tiles: Vec<Entry> = Vec::new();
		let mut min_size: u64 = 0;
		let mut size_sum: u64 = 0;
		let mut tile_count: u64 = 0;
		let mut level_stats: Vec<(u8, u64, u64)> = Vec::new();

		let total_tiles = self.metadata().bbox_pyramid.count_tiles();
		let progress = runtime.create_progress("scanning tiles", total_tiles);

		for bbox in self.metadata().bbox_pyramid.iter_bboxes() {
			let mut level_size_sum: u64 = 0;
			let mut level_count: u64 = 0;
			let mut stream = self.tile_size_stream(bbox).await?;
			while let Some((coord, size_u32)) = stream.next().await {
				let size = u64::from(size_u32);

				tile_count += 1;
				size_sum += size;
				level_size_sum += size;
				level_count += 1;
				progress.inc(1);

				if size < min_size {
					continue;
				}

				let pos = biggest_tiles
					.binary_search_by(|e| e.size.cmp(&size).reverse())
					.unwrap_or_else(|p| p);
				biggest_tiles.insert(
					pos,
					Entry {
						size,
						x: coord.x,
						y: coord.y,
						z: coord.level,
					},
				);
				if biggest_tiles.len() > 10 {
					biggest_tiles.pop();
				}
				min_size = biggest_tiles.last().expect("biggest_tiles is non-empty").size;
			}
			level_stats.push((bbox.level(), level_count, level_size_sum));
		}
		progress.finish();

		if tile_count > 0 {
			print.add_key_value("tile count", &tile_count).await;
			print
				.add_key_value("average tile size", &size_sum.div_euclid(tile_count))
				.await;

			let rows: Vec<Vec<String>> = biggest_tiles
				.iter()
				.enumerate()
				.map(|(i, e)| {
					vec![
						format!("{}", i + 1),
						format!("{}", e.z),
						format!("{}", e.x),
						format!("{}", e.y),
						format_integer_str(e.size),
					]
				})
				.collect();
			print
				.add_table("biggest tiles", &["#", "z", "x", "y", "size"], &rows)
				.await;

			let rows: Vec<Vec<String>> = level_stats
				.iter()
				.map(|(level, count, size)| {
					let avg = if *count > 0 { size / count } else { 0 };
					vec![
						format!("{level}"),
						format_integer_str(*count),
						format_integer_str(*size),
						format_integer_str(avg),
					]
				})
				.collect();
			print
				.add_table(
					"tile size analysis per level",
					&["level", "count", "size_sum", "avg_size"],
					&rows,
				)
				.await;
		} else {
			print.add_warning("no tiles found").await;
		}

		Ok(())
	}

	/// Writes sample tile content diagnostics or a placeholder if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tile_contents(&self, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
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
				metadata: TileSourceMetadata {
					bbox_pyramid: TilePyramid::new_full_up_to(3),
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

		async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
			log::trace!("test_source::tile_stream {bbox:?}");
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
		assert_eq!(metadata.bbox_pyramid.level_min().unwrap(), 0);
		assert_eq!(metadata.bbox_pyramid.level_max().unwrap(), 3);
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

	#[tokio::test]
	async fn test_probe_tile_contents() -> Result<()> {
		#[cfg(feature = "cli")]
		{
			use versatiles_core::utils::PrettyPrint;

			let reader = TestReader::new_dummy();
			let mut print = PrettyPrint::new();
			let runtime = TilesRuntime::default();
			reader
				.probe_tile_contents(&mut print.get_category("tile contents").await, &runtime)
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
		let mut print = PrettyPrint::new();
		let runtime = TilesRuntime::default();
		reader.probe_container(&mut print, &runtime).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_tiles() -> Result<()> {
		let reader = TestReader::new_dummy();
		let mut print = PrettyPrint::new();
		let runtime = TilesRuntime::default();
		reader.probe_tiles(&mut print, &runtime).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_all_levels() -> Result<()> {
		let reader = TestReader::new_dummy();
		let runtime = TilesRuntime::default();
		reader.probe(ProbeDepth::Container, &runtime).await?;
		reader.probe(ProbeDepth::Tiles, &runtime).await?;
		reader.probe(ProbeDepth::TileContents, &runtime).await?;
		Ok(())
	}
}
