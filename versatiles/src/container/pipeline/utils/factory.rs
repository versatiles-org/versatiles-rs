use super::{
	super::operations as op, OperationTrait, ReadOperationFactoryTrait,
	TransformOperationFactoryTrait,
};
use crate::utils::vpl::{parse_vpl, VPLNode, VPLPipeline};
use anyhow::{anyhow, Result};
use itertools::Itertools;
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};

pub struct PipelineFactory {
	read_ops: HashMap<String, Box<dyn ReadOperationFactoryTrait>>,
	tran_ops: HashMap<String, Box<dyn TransformOperationFactoryTrait>>,
	dir: PathBuf,
}

impl PipelineFactory {
	pub fn new(dir: &Path) -> Self {
		PipelineFactory {
			read_ops: HashMap::new(),
			tran_ops: HashMap::new(),
			dir: dir.to_path_buf(),
		}
	}

	pub fn default(dir: &Path) -> Self {
		let mut factory = PipelineFactory::new(dir);

		factory.add_read_factory(Box::new(op::dummy_tiles::Factory {}));
		factory.add_read_factory(Box::new(op::read::Factory {}));
		factory.add_read_factory(Box::new(op::overlay_tiles::Factory {}));

		factory.add_tran_factory(Box::new(op::vectortiles_update_properties::Factory {}));

		factory
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

		factory.build(node, &self).await
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

		factory.build(node, source, &self).await
	}

	pub fn resolve_filename(&self, filename: &str) -> String {
		String::from(self.resolve_path(filename).to_str().unwrap())
	}

	pub fn resolve_path(&self, filename: &str) -> PathBuf {
		self.dir.join(filename)
	}

	pub fn get_docs(&self) -> String {
		vec![
			include_str!("help.md").to_string(),
			String::from("---\n# READ operations"),
			self
				.read_ops
				.values()
				.sorted_by_cached_key(|f| f.get_tag_name().to_string())
				.map(|f| format!("## {}\n{}\n\n", f.get_tag_name(), f.get_docs()))
				.join(""),
			String::from("---\n# TRANSFORM operations"),
			self
				.tran_ops
				.values()
				.sorted_by_cached_key(|f| f.get_tag_name().to_string())
				.map(|f| format!("## {}\n{}\n\n", f.get_tag_name(), f.get_docs()))
				.join(""),
		]
		.join("\n")
	}
}
