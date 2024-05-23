use anyhow::{anyhow, ensure, Context, Result};
use yaml_rust2::{Yaml, YamlLoader};

pub struct YamlWrapper {
	yaml: Yaml,
}

impl YamlWrapper {
	fn new(yaml: &Yaml) -> Result<YamlWrapper> {
		Ok(YamlWrapper { yaml: yaml.to_owned() })
	}
	pub fn from_str(text: &str) -> Result<YamlWrapper> {
		let yaml = YamlLoader::load_from_str(text)?;
		ensure!(yaml.len() > 0, "YAML is empty");
		ensure!(yaml.len() == 1, "YAML contains multiple documents");
		YamlWrapper::new(yaml.get(0).unwrap())
	}
	pub fn is_hash(&self) -> bool {
		self.yaml.as_hash().is_some()
	}
	pub fn is_array(&self) -> bool {
		self.yaml.as_vec().is_some()
	}
	fn hash_get(&self, key: &str) -> Result<&Yaml> {
		self
			.yaml
			.as_hash()
			.context("must be an object")?
			.get(&Yaml::from_str(key))
			.ok_or(anyhow!("no entry '{key}' found"))
	}
	pub fn hash_get_value(&self, key: &str) -> Result<YamlWrapper> {
		YamlWrapper::new(self.hash_get(key)?)
	}
	pub fn as_str(&self) -> Result<&str> {
		self.yaml.as_str().ok_or(anyhow!("value be a string"))
	}
	pub fn hash_get_str(&self, key: &str) -> Result<&str> {
		self
			.hash_get(key)?
			.as_str()
			.ok_or(anyhow!("value of '{key}' must be a string"))
	}
	pub fn hash_get_string(&self, key: &str) -> Result<String> {
		Ok(self.hash_get_str(key)?.to_string())
	}
	pub fn hash_get_bool(&self, key: &str) -> Result<bool> {
		self
			.hash_get(key)?
			.as_bool()
			.ok_or(anyhow!("value of '{key}' must be a boolean"))
	}
	pub fn hash_get_as_vec(&self) -> Result<Vec<(String, YamlWrapper)>> {
		self
			.yaml
			.as_hash()
			.context("must be an object")?
			.iter()
			.map(|(key, value)| -> Result<(String, YamlWrapper)> {
				Ok((
					key.as_str().context("key must be a string")?.to_string(),
					YamlWrapper::new(value)?,
				))
			})
			.collect()
	}
	pub fn hash_has_key(&self, key: &str) -> bool {
		self.yaml.as_hash().unwrap().contains_key(&Yaml::from_str(key))
	}
	pub fn array_get_as_vec(&self) -> Result<Vec<YamlWrapper>> {
		self
			.yaml
			.as_vec()
			.context("must be an array")?
			.into_iter()
			.map(|y| YamlWrapper::new(y))
			.collect()
	}
}
