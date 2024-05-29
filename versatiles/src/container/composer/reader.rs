use super::{lookup::TileComposerOperationLookup, operations::TileComposerOperation};
use crate::{
	container::{TilesReader, TilesReaderParameters},
	io::DataReader,
	types::{Blob, TileBBox, TileCompression, TileCoord3, TileStream},
	utils::YamlWrapper,
};
use anyhow::{anyhow, ensure, Context, Result};
use async_trait::async_trait;
use std::{path::Path, str::FromStr};

/// The `TileComposerReader` struct is responsible for managing the tile reading process,
/// applying operations, and returning the composed tiles.
pub struct TileComposerReader {
	pub name: String,
	pub output: Box<dyn TileComposerOperation>,
	pub parameters: TilesReaderParameters,
}

#[allow(dead_code)]
impl TileComposerReader {
	/// Opens a TileComposerReader from a YAML file path.
	///
	/// # Arguments
	///
	/// * `path` - The path to the YAML file.
	///
	/// # Returns
	///
	/// * `Result<TileComposerReader>` - The constructed TileComposerReader or an error if the configuration is invalid.
	pub async fn open_path(path: &Path) -> Result<TileComposerReader> {
		let yaml =
			std::fs::read_to_string(path).with_context(|| anyhow!("Failed to open {path:?}"))?;
		Self::from_str(&yaml, path.to_str().unwrap(), path.parent().unwrap())
			.await
			.with_context(|| format!("failed parsing {path:?} as YAML"))
	}

	/// Opens a TileComposerReader from a DataReader.
	///
	/// # Arguments
	///
	/// * `reader` - The DataReader containing the YAML configuration.
	///
	/// # Returns
	///
	/// * `Result<TileComposerReader>` - The constructed TileComposerReader or an error if the configuration is invalid.
	pub async fn open_reader(mut reader: DataReader, path: &Path) -> Result<TileComposerReader> {
		let yaml = reader.read_all().await?.into_string();
		Self::from_str(&yaml, reader.get_name(), path)
			.await
			.with_context(|| format!("failed parsing {} as YAML", reader.get_name()))
	}

	#[cfg(test)]
	pub async fn open_str(yaml: &str, path: &Path) -> Result<TileComposerReader> {
		Self::from_str(yaml, "from str", path)
			.await
			.with_context(|| format!("failed parsing '{yaml}' as YAML"))
	}

	async fn from_str(yaml: &str, name: &str, path: &Path) -> Result<TileComposerReader> {
		let yaml =
			YamlWrapper::from_str(yaml).with_context(|| format!("failed parsing '{yaml}' as YAML"))?;

		ensure!(yaml.is_hash(), "YAML must be an object");

		let mut lookup =
			TileComposerOperationLookup::from_yaml(yaml.hash_get_value("operations")?, path)?;

		let operation = yaml
			.hash_get_value("output")
			.context("failed parsing output")?;
		let operation = operation.as_str().context("failed parsing output")?;
		let operation = lookup.construct(operation).await?;
		let parameters = operation.get_parameters().clone();

		Ok(TileComposerReader {
			name: name.to_string(),
			output: operation,
			parameters,
		})
	}
}

#[async_trait]
impl TilesReader for TileComposerReader {
	/// Get the name of the reader source, e.g., the filename.
	fn get_name(&self) -> &str {
		&self.name
	}

	/// Get the container name, e.g., versatiles, mbtiles, etc.
	fn get_container_name(&self) -> &str {
		"composer"
	}

	/// Get the reader parameters.
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Override the tile compression.
	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("you can't override the compression of tile composer sources")
	}

	/// Get the metadata, always uncompressed.
	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(None)
	}

	/// Get tile data for the given coordinate, always compressed and formatted.
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		self.output.get_tile_data(coord).await
	}

	/// Get a stream of tiles within the bounding box.
	async fn get_bbox_tile_stream(&mut self, bbox: TileBBox) -> TileStream {
		self.output.get_bbox_tile_stream(bbox).await
	}
}

impl std::fmt::Debug for TileComposerReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileComposerReader")
			.field("name", &self.name)
			.field("parameters", &self.parameters)
			.field("output", &self.output)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::MockTilesWriter;

	fn get_yaml() -> String {
		String::from(
			r"
operations:
  berlin:
    action: read
    filename: berlin.pmtiles
output: berlin
",
		)
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
	async fn open_yaml_str() -> Result<()> {
		let mut reader = TileComposerReader::open_str(&get_yaml(), Path::new("../testdata/")).await?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_composer_reader_open_path() -> Result<()> {
		let path = Path::new("../testdata/composer.yaml");
		let result = TileComposerReader::open_path(path).await;
		assert_eq!(
			result.unwrap_err().to_string(),
			"Failed to open \"../testdata/composer.yaml\""
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_composer_reader_get_tile_data() -> Result<()> {
		let mut reader = TileComposerReader::open_str(&get_yaml(), Path::new("../testdata/")).await?;

		let result = reader.get_tile_data(&TileCoord3::new(0, 0, 14)?).await;
		assert_eq!(result?, None);

		let result = reader
			.get_tile_data(&TileCoord3::new(8800, 5377, 14)?)
			.await?
			.unwrap();

		assert_eq!(result.len(), 71480);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_composer_reader_get_bbox_tile_stream() -> Result<()> {
		let mut reader = TileComposerReader::open_str(&get_yaml(), Path::new("../testdata/")).await?;
		let bbox = TileBBox::new(1, 0, 0, 1, 1)?;
		let result_stream = reader.get_bbox_tile_stream(bbox).await;
		let result = result_stream.collect().await;

		assert!(!result.is_empty());

		Ok(())
	}
}
