use crate::get_reader;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::path::Path;
use versatiles_core::{io::DataReader, tilejson::TileJSON, *};
use versatiles_pipeline::{OperationTrait, PipelineFactory};

/// The `PipelineReader` struct is responsible for managing the tile reading process,
/// applying operations, and returning the composed tiles.
pub struct PipelineReader {
	pub name: String,
	pub operation: Box<dyn OperationTrait>,
	pub parameters: TilesReaderParameters,
}

#[allow(dead_code)]
impl<'a> PipelineReader {
	/// Opens a PipelineReader from a vpl file path.
	///
	/// # Arguments
	///
	/// * `path` - The path to the vpl file.
	///
	/// # Returns
	///
	/// * `Result<PipelineReader>` - The constructed PipelineReader or an error if the configuration is invalid.
	pub async fn open_path(path: &Path) -> Result<PipelineReader> {
		let vpl = std::fs::read_to_string(path).with_context(|| anyhow!("Failed to open {path:?}"))?;
		Self::from_str(&vpl, path.to_str().unwrap(), path.parent().unwrap())
			.await
			.with_context(|| format!("failed parsing {path:?} as VPL"))
	}

	/// Opens a PipelineReader from a DataReader.
	///
	/// # Arguments
	///
	/// * `reader` - The DataReader containing the vpl configuration.
	///
	/// # Returns
	///
	/// * `Result<PipelineReader>` - The constructed PipelineReader or an error if the configuration is invalid.
	pub async fn open_reader(reader: DataReader, dir: &Path) -> Result<PipelineReader> {
		let vpl = reader.read_all().await?.into_string();
		Self::from_str(&vpl, reader.get_name(), dir)
			.await
			.with_context(|| format!("failed parsing {} as VPL", reader.get_name()))
	}

	#[cfg(test)]
	pub async fn open_str(vpl: &str, dir: &Path) -> Result<PipelineReader> {
		Self::from_str(vpl, "from str", dir)
			.await
			.with_context(|| format!("failed parsing '{vpl}' as VPL"))
	}

	fn from_str(vpl: &'a str, name: &'a str, dir: &'a Path) -> BoxFuture<'a, Result<PipelineReader>> {
		Box::pin(async {
			let callback = Box::new(|filename: String| -> BoxFuture<Result<Box<dyn TilesReaderTrait>>> {
				Box::pin(async move { get_reader(&filename).await })
			});
			let factory = PipelineFactory::default(dir, callback);
			let operation: Box<dyn OperationTrait> = factory.operation_from_vpl(vpl).await?;
			let parameters = operation.parameters().clone();

			Ok(PipelineReader {
				name: name.to_string(),
				operation,
				parameters,
			})
		})
	}
}

#[async_trait]
impl TilesReaderTrait for PipelineReader {
	/// Get the name of the reader source, e.g., the filename.
	fn source_name(&self) -> &str {
		&self.name
	}

	/// Get the container name, e.g., versatiles, mbtiles, etc.
	fn container_name(&self) -> &str {
		"pipeline"
	}

	/// Get the reader parameters.
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Override the tile compression.
	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("you can't override the compression of pipeline")
	}

	fn traversal(&self) -> &Traversal {
		self.operation.traversal()
	}

	/// Get the metadata, always uncompressed.
	fn tilejson(&self) -> &TileJSON {
		self.operation.tilejson()
	}

	/// Get tile data for the given coordinate, always compressed and formatted.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.operation.get_tile_data(coord).await
	}

	/// Get a stream of tiles within the bounding box.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		self.operation.get_tile_stream(bbox).await
	}
}

impl std::fmt::Debug for PipelineReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PipelineReader")
			.field("name", &self.name)
			.field("parameters", &self.parameters)
			.field("output", &self.operation)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::MockTilesWriter;
	use versatiles_core::utils::decompress;

	pub const VPL: &str = include_str!("../../../../testdata/berlin.vpl");

	#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
	async fn open_vpl_str() -> Result<()> {
		let mut reader = PipelineReader::open_str(VPL, Path::new("../testdata/")).await?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_open_path() -> Result<()> {
		let path = Path::new("../testdata/pipeline.vpl");
		let result = PipelineReader::open_path(path).await;
		assert_eq!(
			result.unwrap_err().to_string(),
			"Failed to open \"../testdata/pipeline.vpl\""
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_get_tile_data() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/")).await?;

		let result = reader.get_tile_data(&TileCoord3::new(0, 0, 14)?).await;
		assert_eq!(result?, None);

		let result = decompress(
			reader.get_tile_data(&TileCoord3::new(8800, 5377, 14)?).await?.unwrap(),
			&reader.parameters().tile_compression,
		)?;

		assert_eq!(result.len(), 141385);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_pipeline_reader_get_tile_stream() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/")).await?;
		let bbox = TileBBox::new(1, 0, 0, 1, 1)?;
		let result_stream = reader.get_tile_stream(bbox).await?;
		let result = result_stream.collect().await;

		assert!(!result.is_empty());

		Ok(())
	}

	#[tokio::test]
	async fn test_pipeline_reader_trait_and_debug() -> Result<()> {
		let reader = PipelineReader::open_str(VPL, Path::new("../testdata/")).await?;
		// Trait methods
		assert_eq!(reader.source_name(), "from str");
		assert_eq!(reader.container_name(), "pipeline");
		// Parameters should have at least one bbox level
		assert!(reader.parameters().bbox_pyramid.iter_levels().next().is_some());
		// Debug formatting should include struct name and source
		let debug = format!("{reader:?}");
		assert!(debug.contains("PipelineReader"));
		assert!(debug.contains("from str"));
		Ok(())
	}

	#[tokio::test]
	#[should_panic(expected = "you can't override the compression of pipeline")]
	async fn test_override_compression_panic() {
		let mut reader = PipelineReader::open_str(VPL, Path::new("../testdata/")).await.unwrap();
		// override_compression should panic
		reader.override_compression(TileCompression::Uncompressed);
	}
}
