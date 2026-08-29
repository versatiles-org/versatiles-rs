//! Pipeline tile reader.
//!
//! This module defines [`PipelineReader`], a `TileSource` implementation that
//! loads a VersaTiles Pipeline Language (VPL) description, builds the operation
//! graph via [`PipelineFactory`], and exposes a unified tile-reading interface.
//! It supports opening from paths or arbitrary [`DataReader`]s, validates and
//! executes the configured operations, and streams tiles for a given bbox.

use std::{path::Path, sync::Arc};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::future::BoxFuture;
use versatiles_container::{
	SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesReader, TilesRuntime,
};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileStream, io::DataReader};
use versatiles_derive::context;

use crate::{
	PipelineFactory,
	vpl::{VPLNode, VPLPipeline, parse_vpl},
};

/// Tile reader that executes a VPL-defined operation pipeline and returns composed tiles.
///
/// `PipelineReader` owns the parsed operation graph (`operation`) and exposes
/// `TileSource` so it can be used like any other container reader. It can be
/// constructed from a file path, from any [`DataReader`], or (in tests) from a raw string.
/// The `parameters` reported by the reader originate from the pipeline’s output operation
/// and govern traversal, tile format, compression, and metadata.
pub struct PipelineReader {
	name: String,
	operation: Box<dyn TileSource>,
}

impl<'a> PipelineReader {
	/// Opens a `PipelineReader` from a VPL file on disk.
	///
	/// Reads the file, builds the operation graph with [`PipelineFactory::new_runtime_reader`],
	/// and returns a ready-to-use reader. Errors include contextual messages via `#[context]`.
	#[context("opening VPL path '{}'", path.display())]
	pub async fn open(path: &Path, runtime: TilesRuntime) -> Result<PipelineReader> {
		let vpl = std::fs::read_to_string(path).with_context(|| anyhow!("Failed to open {path:?}"))?;
		let path_str = path
			.to_str()
			.ok_or_else(|| anyhow!("path '{}' is not valid UTF-8", path.display()))?;
		let parent = path
			.parent()
			.ok_or_else(|| anyhow!("path '{}' has no parent directory", path.display()))?;
		Self::from_str(&vpl, path_str, parent, runtime)
			.await
			.with_context(|| format!("failed parsing {path:?} as VPL"))
	}

	/// Opens a `PipelineReader` from an arbitrary [`DataReader`] containing VPL.
	///
	/// Useful when VPL is packaged in other containers or fetched over the network.
	#[context("opening VPL from reader '{}'", reader.name())]
	pub async fn open_data(reader: DataReader, dir: &Path, runtime: TilesRuntime) -> Result<PipelineReader> {
		let vpl = reader.read_all().await?.into_string();
		Self::from_str(&vpl, reader.name(), dir, runtime)
			.await
			.with_context(|| format!("failed parsing {} as VPL", reader.name()))
	}

	/// Test helper: constructs a `PipelineReader` from a raw VPL string.
	#[context("opening VPL from string")]
	pub async fn open_str(vpl: &str, dir: &Path, runtime: TilesRuntime) -> Result<PipelineReader> {
		Self::from_str(vpl, "from str", dir, runtime).await
	}

	/// Constructs a `PipelineReader` from a pre-parsed [`VPLPipeline`].
	///
	/// Useful for building pipelines programmatically without going through VPL text format.
	#[context("building pipeline")]
	pub async fn from_pipeline(
		pipeline: VPLPipeline,
		name: &str,
		dir: &Path,
		runtime: TilesRuntime,
	) -> Result<PipelineReader> {
		let factory = PipelineFactory::new_runtime_reader(dir, runtime);
		let operation = factory.build_pipeline(pipeline).await?;
		Ok(PipelineReader {
			name: name.to_string(),
			operation,
		})
	}

	/// Builds `tail` onto an already-built `head`, for an editor rebuilding only what changed.
	///
	/// [`from_pipeline`](Self::from_pipeline) rebuilds every node, and the read node is where
	/// essentially all the cost is — a transform only parses its parameters and wraps its input,
	/// while a read node scales with its input, seconds for a large CSV or GeoJSON. An editor
	/// that rebuilds after each keystroke pays that again for an edit that cannot have changed
	/// it. Splitting a pipeline with [`VPLPipeline::split`](crate::VPLPipeline::split), keeping
	/// the head as a [`SharedTileSource`] and passing it here makes the rebuild cost
	/// proportional to the edit instead. See issues #259 and #262.
	///
	/// `head` is taken by value, so a caller keeping it across rebuilds passes a clone of its
	/// `Arc`; the tile source itself is not cloned. `tail` must contain transform nodes only —
	/// a read node in it is an error, the same as it would be mid-pipeline.
	#[context("building pipeline from parts")]
	pub async fn from_parts(
		head: SharedTileSource,
		tail: Vec<VPLNode>,
		name: &str,
		dir: &Path,
		runtime: TilesRuntime,
	) -> Result<PipelineReader> {
		let factory = PipelineFactory::new_runtime_reader(dir, runtime);
		let mut operation: Box<dyn TileSource> = Box::new(head);
		for node in tail {
			operation = factory.tran_operation_from_node(node, operation).await?;
		}
		Ok(PipelineReader {
			name: name.to_string(),
			operation,
		})
	}

	/// Internal constructor that parses VPL and delegates to [`from_pipeline`](Self::from_pipeline).
	fn from_str(
		vpl: &'a str,
		name: &'a str,
		dir: &'a Path,
		runtime: TilesRuntime,
	) -> BoxFuture<'a, Result<PipelineReader>> {
		Box::pin(async move {
			let pipeline = parse_vpl(vpl)?;
			Self::from_pipeline(pipeline, name, dir, runtime).await
		})
	}
}

#[async_trait]
impl TilesReader for PipelineReader {
	async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<SharedTileSource> {
		Ok(Self::open(path, runtime).await?.into_shared())
	}

	async fn open_reader(reader: DataReader, runtime: TilesRuntime) -> Result<SharedTileSource> {
		let dir = std::env::current_dir()?;
		Ok(Self::open_data(reader, &dir, runtime).await?.into_shared())
	}
}

#[async_trait]
impl TileSource for PipelineReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("pipeline", SourceType::new_container("replace", "me"))
	}

	/// Returns the reader parameters (tile format, compression, and traversal hints).
	fn metadata(&self) -> &TileSourceMetadata {
		self.operation.metadata()
	}

	/// Returns the pipeline’s tile metadata (`TileJSON`), always uncompressed.
	fn tilejson(&self) -> &TileJSON {
		self.operation.tilejson()
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.operation.tile_pyramid().await
	}

	/// Fetches a single tile for `coord` by executing the pipeline over that tile’s bbox.
	///
	/// Returns `Ok(None)` if the pipeline yields no tile.
	///
	/// Forwarded to the operation's own `tile` rather than taking the first
	/// item of a one-tile stream. An operation is allowed to answer a single
	/// tile differently from a bulk read — `raster_overview` puts a ceiling on
	/// how much of the pyramid one request may build, which is the difference
	/// between a slow response and a server spending minutes on a tile nobody
	/// is waiting for any more — and going around it would silently disable
	/// every such implementation.
	#[context("getting tile {:?} via pipeline '{}'", coord, self.name)]
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		self.operation.tile(coord).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.operation.tile_coord_stream(bbox).await
	}

	async fn tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		self.operation.tile_size_stream(bbox).await
	}

	/// Streams all tiles intersecting `bbox` by executing the pipeline's output operation.
	#[context("streaming tiles for bbox {:?} via pipeline '{}'", bbox, self.name)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("pipeline_reader::tile_stream {bbox:?}");
		self.operation.tile_stream(bbox).await
	}
}

impl std::fmt::Debug for PipelineReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PipelineReader")
			.field("name", &self.name)
			.field("output", &self.operation)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;
	use versatiles_container::MockWriter;
	use versatiles_core::{TileCompression, TileCoord};

	use super::*;

	pub const VPL: &str = include_str!("../../../testdata/berlin.vpl");

	#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
	async fn open_vpl_str() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;
		MockWriter::write(&reader).await?;

		Ok(())
	}

	/// `from_parts` must produce what [`PipelineReader::from_pipeline`] produces for the same
	/// pipeline — that is the whole point of it: an editor swaps the cheap path in without the
	/// result changing. See issue #262.
	#[tokio::test]
	async fn from_parts_matches_from_pipeline() -> Result<()> {
		const VPL: &str = "from_container filename=\"berlin.mbtiles\" | filter level_min=10 level_max=12";
		let dir = Path::new("../testdata");

		let whole = PipelineReader::from_pipeline(parse_vpl(VPL)?, "whole", dir, TilesRuntime::new_silent()).await?;

		let factory = PipelineFactory::new_runtime_reader(dir, TilesRuntime::new_silent());
		let (head, tail) = parse_vpl(VPL)?.split()?;
		let head: SharedTileSource = factory.read_operation_from_node(head).await?.into();

		let parts = PipelineReader::from_parts(head.clone(), tail, "parts", dir, TilesRuntime::new_silent()).await?;

		assert_eq!(parts.source_type(), whole.source_type());
		assert_eq!(parts.metadata(), whole.metadata());
		assert_eq!(parts.tilejson().stringify(), whole.tilejson().stringify());
		// a tile inside the Berlin bounds, so this compares content rather than two `None`s
		let coord = TileCoord::new(12, 2200, 1343)?;
		let tile = parts.tile(&coord).await?;
		assert!(tile.is_some(), "expected a tile at {coord:?}");
		assert_eq!(tile, whole.tile(&coord).await?);

		// the caller keeps its head: `from_parts` took a clone of the `Arc`, not the source
		assert!(!head.tile_pyramid().await?.is_empty());

		Ok(())
	}

	/// A read node in `tail` is an error, the same as it would be mid-pipeline — `from_parts`
	/// folds transforms only.
	#[tokio::test]
	async fn from_parts_rejects_a_read_node_in_the_tail() -> Result<()> {
		let dir = Path::new("../testdata");
		let factory = PipelineFactory::new_runtime_reader(dir, TilesRuntime::new_silent());
		let (head, _) = parse_vpl("from_container filename=\"berlin.mbtiles\"")?.split()?;
		let head: SharedTileSource = factory.read_operation_from_node(head).await?.into();

		let tail = vec![parse_vpl("from_debug format=png")?.split()?.0];
		let error = PipelineReader::from_parts(head, tail, "parts", dir, TilesRuntime::new_silent())
			.await
			.unwrap_err();

		assert!(
			error
				.chain()
				.any(|e| e.to_string() == "transform operation 'from_debug' unknown"),
			"expected an unknown-transform error, got: {error:?}"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_open_path() -> Result<()> {
		let path = Path::new("../testdata/pipeline.vpl");
		let result = PipelineReader::open(path, TilesRuntime::new_silent()).await;
		assert_eq!(
			result
				.unwrap_err()
				.chain()
				.map(std::string::ToString::to_string)
				.collect::<Vec<_>>()[0..2],
			[
				"opening VPL path '../testdata/pipeline.vpl'",
				"Failed to open \"../testdata/pipeline.vpl\"",
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_tile() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;

		let result = reader.tile(&TileCoord::new(14, 0, 0)?).await;
		assert_eq!(result?, None);

		let result = reader
			.tile(&TileCoord::new(14, 8800, 5377)?)
			.await?
			.unwrap()
			.into_blob(&TileCompression::Uncompressed)?;

		assert_eq!(result.len(), 187477);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_tile_stream() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let result_stream = reader.tile_stream(bbox).await?;
		let result = result_stream.to_vec().await;

		assert!(!result.is_empty());

		Ok(())
	}

	#[tokio::test]
	async fn test_pipeline_reader_trait_and_debug() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;
		// Trait methods
		assert_eq!(reader.source_type().to_string(), "processor 'pipeline'");
		// Parameters should have at least one bbox level
		assert!(reader.tile_pyramid().await?.iter().next().is_some());
		// Debug formatting should include struct name and source
		let debug = format!("{reader:?}");
		assert!(debug.contains("PipelineReader"));
		assert!(debug.contains("from str"));
		Ok(())
	}
}
