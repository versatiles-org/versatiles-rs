use super::{
	BuilderTrait, ComposerBuilderTrait, OperationTrait, ReaderBuilderTrait, TransformerBuilderTrait,
};
use crate::{
	container::pipeline::operations::{COMPOSERS, READERS, TRANSFORMERS},
	utils::YamlWrapper,
};
use anyhow::{anyhow, ensure, Result};
use std::{
	collections::{HashMap, VecDeque},
	path::{Path, PathBuf},
};

pub struct Factory {
	path: PathBuf,
	lookup_composers: HashMap<String, &'static Box<dyn ComposerBuilderTrait>>,
	lookup_readers: HashMap<String, &'static Box<dyn ReaderBuilderTrait>>,
	lookup_transformers: HashMap<String, &'static Box<dyn TransformerBuilderTrait>>,
}

impl Factory {
	pub fn new(path: &Path) -> Self {
		fn build_lookup<T>(
			i: impl Iterator<Item = &'static Box<T>>,
		) -> HashMap<String, &'static Box<T>>
		where
			T: BuilderTrait + ?Sized + 'static,
		{
			HashMap::from_iter(i.map(|e| (e.get_id().to_string(), e)))
		}

		Self {
			lookup_composers: build_lookup(COMPOSERS.iter()),
			lookup_readers: build_lookup(READERS.iter()),
			lookup_transformers: build_lookup(TRANSFORMERS.iter()),
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
