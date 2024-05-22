use super::{operations::VirtualTileOperation, output::VirtualTilesOutput};
use crate::{
	container::{
		getters::get_simple_reader, r#virtual::operations::new_virtual_tile_operation, TilesReader, TilesReaderParameters,
	},
	types::{Blob, DataReader, TileCompression, TileCoord3, TileFormat},
	utils::YamlWrapper,
};
use anyhow::{bail, ensure, Context, Result};
use axum::async_trait;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::Mutex;

pub type VReader = Arc<Mutex<Box<dyn TilesReader>>>;
pub type VOperation = Arc<Box<dyn VirtualTileOperation>>;

pub struct VirtualTilesReader {
	inputs: HashMap<String, VReader>,
	operations: HashMap<String, VOperation>,
	output: Vec<VirtualTilesOutput>,
	tile_compression: TileCompression,
	tile_format: TileFormat,
	name: String,
}

impl VirtualTilesReader {
	pub async fn open_path(path: &Path) -> Result<VirtualTilesReader> {
		let yaml = std::fs::read_to_string(path)?;
		Self::from_str(&yaml, path.to_str().unwrap())
			.await
			.with_context(|| format!("while parsing {path:?}"))
	}

	pub async fn open_reader(mut reader: DataReader) -> Result<VirtualTilesReader> {
		let yaml = reader.read_all().await?.into_string();
		Self::from_str(&yaml, reader.get_name())
			.await
			.with_context(|| format!("while parsing {}", reader.get_name()))
	}

	async fn from_str(yaml: &str, name: &str) -> Result<VirtualTilesReader> {
		let yaml = YamlWrapper::from_str(yaml)?;

		ensure!(yaml.is_hash(), "YAML must be an object");

		let (tile_compression, tile_format) = parse_parameters(&yaml.hash_get_value("parameters")?)
			.await
			.context("while parsing 'parameters'")?;

		let inputs = parse_inputs(&yaml.hash_get_value("inputs")?)
			.await
			.context("while parsing 'inputs'")?;

		let operations = if yaml.hash_has_key("operations") {
			parse_operations(&yaml.hash_get_value("operations")?).context("while parsing 'operations'")?
		} else {
			HashMap::new()
		};

		let output =
			parse_output(&yaml.hash_get_value("output")?, &inputs, &operations).context("while parsing 'output'")?;

		Ok(VirtualTilesReader {
			inputs,
			operations,
			output,
			tile_compression,
			tile_format,
			name: name.to_string(),
		})
	}
}

async fn parse_parameters(yaml: &YamlWrapper) -> Result<(TileCompression, TileFormat)> {
	ensure!(yaml.is_hash(), "'parameters' must be an object");
	Ok((
		TileCompression::from_str(yaml.hash_get_str("compression")?)?,
		TileFormat::from_str(yaml.hash_get_str("format")?)?,
	))
}

async fn parse_inputs(yaml: &YamlWrapper) -> Result<HashMap<String, VReader>> {
	ensure!(yaml.is_hash(), "'inputs' must be an object");

	let mut inputs: HashMap<String, VReader> = HashMap::new();

	for (name, entry) in yaml.hash_get_as_vec()? {
		let filename = entry.hash_get_str("filename")?;
		if inputs.contains_key(&name) {
			bail!("input '{name}' is duplicated")
		}
		inputs.insert(name, Arc::new(Mutex::new(get_simple_reader(filename).await?)));
	}

	if inputs.is_empty() {
		bail!("YAML needs at least one input")
	}

	Ok(inputs)
}

fn parse_operations(yaml: &YamlWrapper) -> Result<HashMap<String, VOperation>> {
	ensure!(yaml.is_hash(), "'operations' must be an object");

	let mut operations: HashMap<String, VOperation> = HashMap::new();

	for (index, (name, entry)) in yaml.hash_get_as_vec()?.iter().enumerate() {
		operations.insert(
			name.to_string(),
			Arc::new(
				new_virtual_tile_operation(entry).with_context(|| format!("while parsing operation no {}", index + 1))?,
			),
		);
	}

	Ok(operations)
}

fn parse_output(
	yaml: &YamlWrapper, inputs: &HashMap<String, VReader>, operations: &HashMap<String, VOperation>,
) -> Result<Vec<VirtualTilesOutput>> {
	ensure!(yaml.is_array(), "'output' must be an array");

	let mut output: Vec<VirtualTilesOutput> = Vec::new();

	for (index, entry) in yaml.array_get_as_vec()?.iter().enumerate() {
		output.push(
			VirtualTilesOutput::new(entry, inputs, operations)
				.with_context(|| format!("while parsing output no {}", index + 1))?,
		);
	}

	Ok(output)
}

#[async_trait]
impl TilesReader for VirtualTilesReader {
	#[doc = "Get the name of the reader source, e.g., the filename."]
	fn get_name(&self) -> &str {
		&self.name
	}

	#[doc = "Get the container name, e.g., versatiles, mbtiles, etc."]
	fn get_container_name(&self) -> &str {
		"virtual"
	}

	#[doc = "Get the reader parameters."]
	fn get_parameters(&self) -> &TilesReaderParameters {
		todo!()
	}

	#[doc = "Override the tile compression."]
	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("you can't override the compression of virtual tile sources")
	}

	#[doc = "Get the metadata, always uncompressed."]
	fn get_meta(&self) -> Result<Option<Blob>> {
		todo!()
	}

	#[doc = "Get tile data for the given coordinate, always compressed and formatted."]
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		todo!()
	}
}

impl std::fmt::Debug for VirtualTilesReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("VirtualTilesReader")
			//.field("parameters", &self.get_parameters())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn open_yaml() -> Result<()> {
		let reader = VirtualTilesReader::open_path(&Path::new("testdata/test.yaml")).await?;
		println!("{reader:?}");
		Ok(())
	}
}
