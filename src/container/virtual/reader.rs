use crate::{
	container::{getters::get_simple_reader, TilesReader, TilesReaderParameters},
	types::{Blob, DataReader, TileCompression, TileCoord3},
};
use anyhow::{anyhow, bail, Context, Result};
use axum::async_trait;
use std::{collections::HashMap, path::Path};
use yaml_rust2::{Yaml, YamlLoader};

pub struct VirtualTilesReader {
	inputs: HashMap<String, Box<dyn TilesReader>>,
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
		let yaml = YamlLoader::load_from_str(yaml)?;
		let yaml = yaml.get(0).ok_or(anyhow!("YAML is empty"))?;

		let yaml = yaml.as_hash().ok_or(anyhow!("YAML has no keys"))?;

		let inputs = yaml
			.get(&Yaml::from_str("inputs"))
			.ok_or(anyhow!("YAML needs an entry 'inputs'"))?;
		let inputs = parse_inputs(inputs).await.context("while parsing 'inputs'")?;

		Ok(VirtualTilesReader {
			inputs,
			name: name.to_string(),
		})
	}
}

async fn parse_inputs(yaml: &Yaml) -> Result<HashMap<String, Box<dyn TilesReader>>> {
	let yaml = yaml.as_vec().ok_or(anyhow!("inputs must be an array"))?;
	let mut inputs: HashMap<String, Box<dyn TilesReader>> = HashMap::new();

	for entry in yaml.iter() {
		let name = get_yaml_str(entry, "name")?;
		let filename = get_yaml_str(entry, "filename")?;
		if inputs.contains_key(&name) {
			bail!("input '{name}' is duplicated")
		}
		inputs.insert(name, get_simple_reader(&filename).await?);
	}

	if inputs.is_empty() {
		bail!("YAML needs at least one input")
	}

	Ok(inputs)
}
fn get_yaml_str(yaml: &Yaml, key: &str) -> Result<String> {
	Ok(yaml
		.as_hash()
		.ok_or(anyhow!("entry must be an objects"))?
		.get(&Yaml::from_str(key))
		.ok_or(anyhow!("entry must contain key '{key}'"))?
		.as_str()
		.with_context(|| format!("value of '{key}' must be a string"))?
		.to_string())
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
