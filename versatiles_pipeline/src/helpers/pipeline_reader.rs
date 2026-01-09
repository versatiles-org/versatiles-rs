//! Pipeline tile reader.
//!
//! This module defines [`PipelineReader`], a `TileSource` implementation that
//! loads a VersaTiles Pipeline Language (VPL) description, builds the operation
//! graph via [`PipelineFactory`], and exposes a unified tile-reading interface.
//! It supports opening from paths or arbitrary [`DataReader`]s, validates and
//! executes the configured operations, and streams tiles for a given bbox.

use crate::PipelineFactory;
use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::{path::Path, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TileStream, io::DataReader};
use versatiles_derive::context;

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

#[allow(dead_code)]
impl<'a> PipelineReader {
	/// Opens a `PipelineReader` from a VPL file on disk.
	///
	/// Reads the file, builds the operation graph with [`PipelineFactory::new_default`],
	/// and returns a ready-to-use reader. Errors include contextual messages via `#[context]`.
	#[context("opening VPL path '{}'", path.display())]
	pub async fn open_path(path: &Path, runtime: TilesRuntime) -> Result<PipelineReader> {
		let vpl = std::fs::read_to_string(path).with_context(|| anyhow!("Failed to open {path:?}"))?;
		Self::from_str(&vpl, path.to_str().unwrap(), path.parent().unwrap(), runtime)
			.await
			.with_context(|| format!("failed parsing {path:?} as VPL"))
	}

	/// Opens a `PipelineReader` from an arbitrary [`DataReader`] containing VPL.
	///
	/// Useful when VPL is packaged in other containers or fetched over the network.
	#[context("opening VPL from reader '{}'", reader.get_name())]
	pub async fn open_reader(reader: DataReader, dir: &Path, runtime: TilesRuntime) -> Result<PipelineReader> {
		let vpl = reader.read_all().await?.into_string();
		Self::from_str(&vpl, reader.get_name(), dir, runtime)
			.await
			.with_context(|| format!("failed parsing {} as VPL", reader.get_name()))
	}

	/// Test helper: constructs a `PipelineReader` from a raw VPL string.
	#[context("opening VPL from string")]
	pub async fn open_str(vpl: &str, dir: &Path, runtime: TilesRuntime) -> Result<PipelineReader> {
		Self::from_str(vpl, "from str", dir, runtime).await
	}

	/// Internal constructor that parses VPL and wires up the callback used by `PipelineFactory`
	/// to resolve nested readers via `ContainerRegistry`.
	fn from_str(
		vpl: &'a str,
		name: &'a str,
		dir: &'a Path,
		runtime: TilesRuntime,
	) -> BoxFuture<'a, Result<PipelineReader>> {
		Box::pin(async move {
			let runtime2 = runtime.clone();
			let callback = Box::new(move |filename: String| -> BoxFuture<Result<Box<dyn TileSource>>> {
				let runtime = runtime2.clone();
				Box::pin(async move {
					let arc_reader = runtime.clone().get_reader_from_str(&filename).await?;
					Arc::try_unwrap(arc_reader)
						.map_err(|_| anyhow::anyhow!("Cannot get exclusive access to reader for pipeline"))
				})
			});
			let factory = PipelineFactory::new_default(dir, callback, runtime);
			let operation: Box<dyn TileSource> = factory.operation_from_vpl(vpl).await?;

			Ok(PipelineReader {
				name: name.to_string(),
				operation,
			})
		})
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

	/// Fetches a single tile for `coord` by executing the pipeline over that tile’s bbox.
	///
	/// Returns `Ok(None)` if the pipeline yields no tile; returns an error if multiple tiles
	/// are produced (pipelines must emit at most one tile per coordinate).
	#[context("getting tile {:?} via pipeline '{}'", coord, self.name)]
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let mut vec = self
			.operation
			.get_tile_stream(coord.to_tile_bbox())
			.await?
			.to_vec()
			.await;

		ensure!(vec.len() <= 1, "PipelineReader should return at most one tile");

		if let Some((_, b)) = vec.pop() {
			Ok(Some(b))
		} else {
			Ok(None)
		}
	}

	/// Streams all tiles intersecting `bbox` by executing the pipeline's output operation.
	#[context("streaming tiles for bbox {:?} via pipeline '{}'", bbox, self.name)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {bbox:?}");
		self.operation.get_tile_stream(bbox).await
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
	use super::*;
	use pretty_assertions::assert_eq;
	use versatiles_container::MockWriter;
	use versatiles_core::{TileCompression, TileCoord};

	pub const VPL: &str = include_str!("../../../testdata/berlin.vpl");

	#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
	async fn open_vpl_str() -> Result<()> {
		let mut reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;
		MockWriter::write(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_open_path() -> Result<()> {
		let path = Path::new("../testdata/pipeline.vpl");
		let result = PipelineReader::open_path(path, TilesRuntime::new_silent()).await;
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
	async fn test_tile_pipeline_reader_get_tile() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;

		let result = reader.get_tile(&TileCoord::new(14, 0, 0)?).await;
		assert_eq!(result?, None);

		let result = reader
			.get_tile(&TileCoord::new(14, 8800, 5377)?)
			.await?
			.unwrap()
			.into_blob(TileCompression::Uncompressed)?;

		assert_eq!(result.len(), 141385);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_get_tile_stream() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/"), TilesRuntime::new_silent()).await?;
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let result_stream = reader.get_tile_stream(bbox).await?;
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
		assert!(reader.metadata().bbox_pyramid.iter_levels().next().is_some());
		// Debug formatting should include struct name and source
		let debug = format!("{reader:?}");
		assert!(debug.contains("PipelineReader"));
		assert!(debug.contains("from str"));
		Ok(())
	}
}
