use super::{operations::VirtualTileOperation, output::VirtualTilesOutput};
use crate::{
	container::{
		getters::get_simple_reader, r#virtual::operations::new_virtual_tile_operation, TilesReader, TilesReaderParameters,
	},
	types::{Blob, DataReader, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat},
	utils::{compress, YamlWrapper},
};
use anyhow::{bail, ensure, Context, Result};
use axum::async_trait;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::Mutex;

pub type VReader = Arc<Mutex<Box<dyn TilesReader>>>;
pub type VOperation = Arc<Box<dyn VirtualTileOperation>>;

pub struct VirtualTilesReader {
	inputs: HashMap<String, VReader>,
	name: String,
	operations: HashMap<String, VOperation>,
	outputs: Vec<VirtualTilesOutput>,
	tiles_reader_parameters: TilesReaderParameters,
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

		let inputs = parse_inputs(&yaml.hash_get_value("inputs")?)
			.await
			.context("while parsing 'inputs'")?;

		let operations = if yaml.hash_has_key("operations") {
			parse_operations(&yaml.hash_get_value("operations")?).context("while parsing 'operations'")?
		} else {
			HashMap::new()
		};

		let outputs = parse_output(&yaml.hash_get_value("output")?, &inputs, &operations)
			.await
			.context("while parsing 'output'")?;

		let tiles_reader_parameters =
			parse_parameters(&yaml.hash_get_value("parameters")?, &outputs).context("while parsing 'parameters'")?;

		Ok(VirtualTilesReader {
			inputs,
			name: name.to_string(),
			operations,
			outputs,
			tiles_reader_parameters,
		})
	}
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

async fn parse_output(
	yaml: &YamlWrapper, input_lookup: &HashMap<String, VReader>, operation_lookup: &HashMap<String, VOperation>,
) -> Result<Vec<VirtualTilesOutput>> {
	ensure!(yaml.is_array(), "'output' must be an array");

	let mut output: Vec<VirtualTilesOutput> = Vec::new();

	for (index, entry) in yaml.array_get_as_vec()?.iter().enumerate() {
		output.push(
			VirtualTilesOutput::new(entry, input_lookup, operation_lookup)
				.await
				.with_context(|| format!("while parsing output no {}", index + 1))?,
		);
	}

	Ok(output)
}

fn parse_parameters(yaml: &YamlWrapper, outputs: &Vec<VirtualTilesOutput>) -> Result<TilesReaderParameters> {
	ensure!(yaml.is_hash(), "'parameters' must be an object");
	let tile_compression = TileCompression::from_str(yaml.hash_get_str("compression")?)?;
	let tile_format = TileFormat::from_str(yaml.hash_get_str("format")?)?;

	let mut bbox_pyramid = TileBBoxPyramid::new_empty();
	for output in outputs.iter() {
		bbox_pyramid.include_bbox_pyramid(&output.bbox_pyramid);
	}

	Ok(TilesReaderParameters {
		bbox_pyramid,
		tile_compression,
		tile_format,
	})
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
		&self.tiles_reader_parameters
	}

	#[doc = "Override the tile compression."]
	fn override_compression(&mut self, _tile_compression: TileCompression) {
		panic!("you can't override the compression of virtual tile sources")
	}

	#[doc = "Get the metadata, always uncompressed."]
	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(None)
	}

	#[doc = "Get tile data for the given coordinate, always compressed and formatted."]
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for output in self.outputs.iter() {
			if !output.bbox_pyramid.contains_coord(coord) {
				continue;
			}
			if let Some(mut tile) = output.get_tile_data(coord).await? {
				tile = compress(tile, &self.tiles_reader_parameters.tile_compression)?;
				return Ok(Some(tile));
			} else {
				continue;
			}
		}
		Ok(None)
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
