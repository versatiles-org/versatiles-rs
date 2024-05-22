use super::VirtualTileOperation;
use crate::utils::YamlWrapper;
use anyhow::Result;

pub struct ReplaceValuesOperation {
	data_source: String,
	id_field_tiles: String,
	id_field_values: String,
}

impl VirtualTileOperation for ReplaceValuesOperation {
	fn new(yaml: &YamlWrapper) -> Result<Box<dyn VirtualTileOperation>>
	where
		Self: Sized,
	{
		Ok(Box::new(ReplaceValuesOperation {
			data_source: yaml.hash_get_string("data_source")?,
			id_field_tiles: yaml.hash_get_string("id_field_tiles")?,
			id_field_values: yaml.hash_get_string("id_field_values")?,
		}))
	}
}
