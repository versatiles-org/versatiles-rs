use crate::{
	operations::{get_read_operation_factories, get_transform_operation_factories},
	traits::{OperationTrait, ReadOperationFactoryTrait, TransformOperationFactoryTrait},
	vpl::{parse_vpl, VPLNode, VPLPipeline},
};
use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use itertools::Itertools;
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};
use versatiles_core::types::TilesReader;

type Callback = Option<Box<dyn Fn(String) -> BoxFuture<'static, Result<Box<dyn TilesReader>>>>>;

pub struct PipelineFactory {
	read_ops: HashMap<String, Box<dyn ReadOperationFactoryTrait>>,
	tran_ops: HashMap<String, Box<dyn TransformOperationFactoryTrait>>,
	dir: PathBuf,
	create_reader: Callback,
}

impl PipelineFactory {
	pub fn new(dir: &Path, create_reader: Callback) -> Self {
		PipelineFactory {
			read_ops: HashMap::new(),
			tran_ops: HashMap::new(),
			dir: dir.to_path_buf(),
			create_reader,
		}
	}

	pub fn default(dir: &Path, create_reader: Callback) -> Self {
		let mut factory = PipelineFactory::new(dir, create_reader);

		for f in get_read_operation_factories() {
			factory.add_read_factory(f)
		}

		for f in get_transform_operation_factories() {
			factory.add_tran_factory(f)
		}

		factory
	}

	#[cfg(test)]
	pub fn dummy() -> Self {
		PipelineFactory::default(&Path::new("/"), None)
	}

	fn add_read_factory(&mut self, factory: Box<dyn ReadOperationFactoryTrait>) {
		self
			.read_ops
			.insert(factory.get_tag_name().to_string(), factory);
	}

	fn add_tran_factory(&mut self, factory: Box<dyn TransformOperationFactoryTrait>) {
		self
			.tran_ops
			.insert(factory.get_tag_name().to_string(), factory);
	}

	pub async fn get_reader(&self, filename: &str) -> Result<Box<dyn TilesReader>> {
		(self.create_reader.as_ref().unwrap())(self.dir.join(filename).to_string_lossy().to_string())
			.await
	}

	pub async fn operation_from_vpl(&self, text: &str) -> Result<Box<dyn OperationTrait>> {
		let pipeline = parse_vpl(text)?;
		self.build_pipeline(pipeline).await
	}

	pub async fn build_pipeline(&self, pipeline: VPLPipeline) -> Result<Box<dyn OperationTrait>> {
		let (head, tail) = pipeline.split()?;

		let mut vpl_operation = self.read_operation_from_node(head).await?;

		for node in tail {
			vpl_operation = self.tran_operation_from_node(node, vpl_operation).await?;
		}

		Ok(vpl_operation)
	}

	async fn read_operation_from_node(&self, node: VPLNode) -> Result<Box<dyn OperationTrait>> {
		let factory = self
			.read_ops
			.get(&node.name)
			.ok_or_else(|| anyhow!("read operation '{}' unknown", node.name))?;

		factory.build(node, self).await
	}

	async fn tran_operation_from_node(
		&self,
		node: VPLNode,
		source: Box<dyn OperationTrait>,
	) -> Result<Box<dyn OperationTrait>> {
		let factory = self
			.tran_ops
			.get(&node.name)
			.ok_or_else(|| anyhow!("transform operation '{}' unknown", node.name))?;

		factory.build(node, source, self).await
	}

	pub fn resolve_filename(&self, filename: &str) -> String {
		String::from(self.resolve_path(filename).to_str().unwrap())
	}

	pub fn resolve_path(&self, filename: &str) -> PathBuf {
		self.dir.join(filename)
	}

	pub fn get_docs(&self) -> String {
		[
			include_str!("help.md").to_string(),
			String::from("---\n# READ operations"),
			self
				.read_ops
				.values()
				.sorted_by_cached_key(|f| f.get_tag_name().to_string())
				.map(|f| format!("\n## {}\n{}\n", f.get_tag_name(), f.get_docs()))
				.join(""),
			String::from("---\n# TRANSFORM operations"),
			self
				.tran_ops
				.values()
				.sorted_by_cached_key(|f| f.get_tag_name().to_string())
				.map(|f| format!("\n## {}\n{}\n", f.get_tag_name(), f.get_docs()))
				.join(""),
		]
		.join("\n")
	}
}

unsafe impl Sync for PipelineFactory {}
unsafe impl Send for PipelineFactory {}
