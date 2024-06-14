use super::{OperationTrait, ReaderBuilderTrait, TransformerBuilderTrait};
use crate::{
	container::pipeline::operations::{READERS, TRANSFORMERS},
	utils::YamlWrapper,
};
use anyhow::{anyhow, ensure, Result};
use std::{
	collections::{HashMap, VecDeque},
	path::{Path, PathBuf},
};

pub struct Factory<'a> {
	path: PathBuf,
	lookup_readers: HashMap<String, &'a Box<dyn ReaderBuilderTrait>>,
	lookup_transformers: HashMap<String, &'a Box<dyn TransformerBuilderTrait>>,
}

impl<'a> Factory<'a> {
	pub fn new(path: &Path) -> Self {
		let mut lookup_readable = HashMap::new();
		for e in READERS.iter() {
			lookup_readable.insert(e.get_id().to_string(), e);
		}

		Self {
			lookup_readers: lookup_readable,
			lookup_transformers: HashMap::from_iter(
				TRANSFORMERS.iter().map(|e| (e.get_id().to_string(), e)),
			),
			path: path.to_owned(),
		}
	}

	pub async fn from_yaml(&self, yaml: YamlWrapper) -> Result<Box<dyn OperationTrait>> {
		ensure!(yaml.is_array(), "must be an array");

		let mut yamls = VecDeque::from(yaml.array_get_as_vec()?);
		ensure!(yamls.len() > 0, "need at least one entry");

		let mut reader = self.readable_from_yaml(yamls.pop_front().unwrap()).await?;
		for yaml in yamls {
			reader = self.transform_from_yaml(yaml, reader).await?;
		}

		Ok(reader)
	}

	async fn readable_from_yaml(&self, yaml: YamlWrapper) -> Result<Box<dyn OperationTrait>> {
		let run = yaml.hash_get_str("run")?;
		let builder = self
			.lookup_readers
			.get(run)
			.ok_or_else(|| anyhow!("Readable '{run}' not found"))?;
		builder.build(yaml, self).await
	}

	async fn transform_from_yaml(
		&self,
		yaml: YamlWrapper,
		reader: Box<dyn OperationTrait>,
	) -> Result<Box<dyn OperationTrait>> {
		let run = yaml.hash_get_str("run")?;
		let builder = self
			.lookup_transformers
			.get(run)
			.ok_or_else(|| anyhow!("Transform '{run}' not found"))?;
		builder.build(yaml, reader, self).await
	}

	pub fn get_absolute_str(&self, filename: &str) -> String {
		self.path.join(filename).to_str().unwrap().to_string()
	}

	pub fn get_path(&self) -> &Path {
		&self.path
	}
}
