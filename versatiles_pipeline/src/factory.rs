use crate::{
	helpers::{dummy_image_source::DummyImageSource, dummy_vector_source::DummyVectorSource},
	operations::{get_read_operation_factories, get_transform_operation_factories},
	traits::{OperationTrait, ReadOperationFactoryTrait, TransformOperationFactoryTrait},
	vpl::{VPLNode, VPLPipeline, parse_vpl},
};
use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use itertools::Itertools;
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};
use versatiles_container::{TilesReaderTrait, WriterConfig};

type Callback = Box<dyn Fn(String) -> BoxFuture<'static, Result<Box<dyn TilesReaderTrait>>>>;

pub struct PipelineFactory {
	read_ops: HashMap<String, Box<dyn ReadOperationFactoryTrait>>,
	tran_ops: HashMap<String, Box<dyn TransformOperationFactoryTrait>>,
	dir: PathBuf,
	create_reader: Callback,
	config: WriterConfig,
}

impl PipelineFactory {
	pub fn new_empty(dir: &Path, create_reader: Callback, config: WriterConfig) -> Self {
		PipelineFactory {
			read_ops: HashMap::new(),
			tran_ops: HashMap::new(),
			dir: dir.to_path_buf(),
			create_reader,
			config,
		}
	}

	pub fn new_default(dir: &Path, create_reader: Callback, config: WriterConfig) -> Self {
		let mut factory = PipelineFactory::new_empty(dir, create_reader, config);

		for f in get_read_operation_factories() {
			factory.add_read_factory(f)
		}

		for f in get_transform_operation_factories() {
			factory.add_tran_factory(f)
		}

		factory
	}

	pub fn new_dummy() -> Self {
		PipelineFactory::new_dummy_reader(Box::new(
			|filename: String| -> BoxFuture<Result<Box<dyn TilesReaderTrait>>> {
				Box::pin(async {
					let filename = filename;
					let extension = filename.split('.').next_back().unwrap().to_string();
					Ok(match extension.as_str() {
						"pbf" | "mvt" => Box::new(DummyVectorSource::new(
							&[("dummy", &[&[("filename", &filename)]])],
							None,
						)) as Box<dyn TilesReaderTrait>,
						"avif" | "png" | "jpg" | "jpeg" | "webp" => {
							Box::new(DummyImageSource::new(&filename, None, 4).unwrap()) as Box<dyn TilesReaderTrait>
						}
						_ => panic!("unknown file extension '{}'", extension),
					})
				})
			},
		))
	}

	pub fn new_dummy_reader(create_reader: Callback) -> Self {
		PipelineFactory::new_default(Path::new(""), create_reader, WriterConfig::default())
	}

	fn add_read_factory(&mut self, factory: Box<dyn ReadOperationFactoryTrait>) {
		self.read_ops.insert(factory.get_tag_name().to_string(), factory);
	}

	fn add_tran_factory(&mut self, factory: Box<dyn TransformOperationFactoryTrait>) {
		self.tran_ops.insert(factory.get_tag_name().to_string(), factory);
	}

	pub async fn get_reader(&self, filename: &str) -> Result<Box<dyn TilesReaderTrait>> {
		(self.create_reader.as_ref())(self.dir.join(filename).to_string_lossy().to_string()).await
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
				.sorted_by_key(|f| f.get_tag_name())
				.map(|f| format!("\n## {}\n{}\n", f.get_tag_name(), f.get_docs()))
				.join(""),
			String::from("---\n# TRANSFORM operations"),
			self
				.tran_ops
				.values()
				.sorted_by_key(|f| f.get_tag_name())
				.map(|f| format!("\n## {}\n{}\n", f.get_tag_name(), f.get_docs()))
				.join(""),
		]
		.join("\n")
	}

	pub fn config(&self) -> &WriterConfig {
		&self.config
	}
}

unsafe impl Sync for PipelineFactory {}
unsafe impl Send for PipelineFactory {}
